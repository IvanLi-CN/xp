import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import { fetchAdminNodes, patchAdminNode } from "../api/adminNodes";
import {
	type AdminQuotaPolicyGlobalWeightRow,
	fetchAdminQuotaPolicyGlobalWeightRows,
	putAdminQuotaPolicyGlobalWeightRow,
} from "../api/adminQuotaPolicyGlobalWeightRows";
import {
	fetchAdminQuotaPolicyNodePolicy,
	putAdminQuotaPolicyNodePolicy,
} from "../api/adminQuotaPolicyNodePolicy";
import {
	type AdminQuotaPolicyNodeWeightRow,
	fetchAdminQuotaPolicyNodeWeightRows,
} from "../api/adminQuotaPolicyNodeWeightRows";
import { putAdminUserNodeWeight } from "../api/adminUserNodeWeights";
import { fetchAdminUsers, patchAdminUser } from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { NodeQuotaEditor } from "../components/NodeQuotaEditor";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { ResourceTable } from "../components/ResourceTable";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";
import {
	RATIO_BASIS_POINTS,
	basisPointsToWeights,
	formatPercentFromBasisPoints,
	parsePercentInput,
	rebalanceAfterEdit,
	weightsToBasisPoints,
} from "../utils/quotaPolicyWeights";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

function formatUtcOffsetMinutes(minutes: number): string {
	const sign = minutes >= 0 ? "+" : "-";
	const abs = Math.abs(minutes);
	const hh = String(Math.floor(abs / 60)).padStart(2, "0");
	const mm = String(abs % 60).padStart(2, "0");
	return `UTC${sign}${hh}:${mm}`;
}

function formatNodeQuotaResetBrief(q: {
	policy: "monthly" | "unlimited";
	day_of_month?: number;
	tz_offset_minutes?: number | null;
}): string {
	const tz =
		q.tz_offset_minutes === null || q.tz_offset_minutes === undefined
			? "(local)"
			: formatUtcOffsetMinutes(q.tz_offset_minutes);
	if (q.policy === "monthly") {
		return `monthly@${q.day_of_month ?? 1} ${tz}`;
	}
	return `unlimited ${tz}`;
}

type RatioDraftRow = {
	userId: string;
	displayName: string;
	priorityTier: "p1" | "p2" | "p3";
	endpointIds: string[];
	basisPoints: number;
	locked: boolean;
	serverStoredWeight: number | null;
	source: "explicit" | "implicit_zero" | "implicit_default";
};

type SaveFailure = {
	userId: string;
	displayName: string;
	targetWeight: number;
	error: string;
};

type LastSaveState = {
	status: "success" | "partial" | "error";
	message: string;
	at: string;
};

type PieSegment = {
	key: string;
	label: string;
	basisPoints: number;
	color: string;
	userIds: string[];
};

const PIE_COLORS = [
	"oklch(var(--p))",
	"oklch(var(--s))",
	"oklch(var(--a))",
	"oklch(var(--in))",
	"oklch(var(--su))",
	"oklch(var(--wa))",
	"oklch(var(--er))",
	"oklch(var(--n))",
] as const;

const RATIO_TABLE_MIN_VIEWPORT = 1024;
const DEFAULT_GLOBAL_WEIGHT = 100;

function pieColorAt(index: number): string {
	return PIE_COLORS[index % PIE_COLORS.length] ?? "oklch(var(--b3))";
}

function stableColorIndex(key: string): number {
	let hash = 0x811c9dc5;
	for (let index = 0; index < key.length; index += 1) {
		hash ^= key.charCodeAt(index);
		hash = Math.imul(hash, 0x01000193);
	}
	return hash >>> 0;
}

function pieColorForKey(key: string): string {
	if (key === "others") return "oklch(var(--b3))";
	return pieColorAt(stableColorIndex(key));
}

function toRadians(angle: number): number {
	return ((angle - 90) * Math.PI) / 180;
}

function polar(cx: number, cy: number, radius: number, angle: number) {
	return {
		x: cx + radius * Math.cos(toRadians(angle)),
		y: cy + radius * Math.sin(toRadians(angle)),
	};
}

function describeArc(
	cx: number,
	cy: number,
	radius: number,
	startAngle: number,
	endAngle: number,
): string {
	const start = polar(cx, cy, radius, endAngle);
	const end = polar(cx, cy, radius, startAngle);
	const largeArc = endAngle - startAngle > 180 ? 1 : 0;
	return [
		`M ${cx} ${cy}`,
		`L ${start.x} ${start.y}`,
		`A ${radius} ${radius} 0 ${largeArc} 0 ${end.x} ${end.y}`,
		"Z",
	].join(" ");
}

function buildPieSegments(rows: RatioDraftRow[]): PieSegment[] {
	const visibleRows = rows.filter((row) => row.basisPoints > 0);
	if (visibleRows.length === 0) {
		return [];
	}

	const maxVisible = 8;
	if (visibleRows.length <= maxVisible) {
		return visibleRows.map((row) => ({
			key: row.userId,
			label: row.displayName,
			basisPoints: row.basisPoints,
			color: pieColorForKey(row.userId),
			userIds: [row.userId],
		}));
	}

	// Keep slice positions stable while editing by preserving row order in the pie.
	const topByWeight = [...visibleRows]
		.sort(
			(a, b) =>
				b.basisPoints - a.basisPoints || a.userId.localeCompare(b.userId),
		)
		.slice(0, maxVisible - 1);
	const topUserIdSet = new Set(topByWeight.map((row) => row.userId));

	const segments: PieSegment[] = [];
	const othersUserIds: string[] = [];
	let othersBasisPoints = 0;

	for (const row of visibleRows) {
		if (topUserIdSet.has(row.userId)) {
			segments.push({
				key: row.userId,
				label: row.displayName,
				basisPoints: row.basisPoints,
				color: pieColorForKey(row.userId),
				userIds: [row.userId],
			});
			continue;
		}
		othersBasisPoints += row.basisPoints;
		othersUserIds.push(row.userId);
	}

	if (othersBasisPoints <= 0) {
		return segments;
	}

	return [
		...segments,
		{
			key: "others",
			label: "Others",
			basisPoints: othersBasisPoints,
			color: pieColorForKey("others"),
			userIds: othersUserIds,
		},
	];
}

function toDraftRows(items: AdminQuotaPolicyNodeWeightRow[]): RatioDraftRow[] {
	const basisPoints = weightsToBasisPoints(
		items.map((item) => item.editor_weight),
	);
	return items.map((item, index) => ({
		userId: item.user_id,
		displayName: item.display_name,
		priorityTier: item.priority_tier,
		endpointIds: item.endpoint_ids,
		basisPoints: basisPoints[index] ?? 0,
		locked: false,
		serverStoredWeight: item.stored_weight ?? null,
		source: item.source,
	}));
}

function toGlobalDraftRows(
	items: AdminQuotaPolicyGlobalWeightRow[],
): RatioDraftRow[] {
	const basisPoints = weightsToBasisPoints(
		items.map((item) => item.editor_weight),
	);
	return items.map((item, index) => ({
		userId: item.user_id,
		displayName: item.display_name,
		priorityTier: item.priority_tier,
		endpointIds: [],
		basisPoints: basisPoints[index] ?? 0,
		locked: false,
		serverStoredWeight: item.stored_weight ?? null,
		source: item.source,
	}));
}

function formatRatioPercent(basisPoints: number): string {
	return `${formatPercentFromBasisPoints(basisPoints)}%`;
}

function useRatioEditorListLayout(minTableViewport: number): boolean {
	const [isListLayout, setIsListLayout] = useState(() => {
		if (typeof window === "undefined") return false;
		if (typeof window.matchMedia === "function") {
			return window.matchMedia(
				`(max-width: ${Math.max(0, minTableViewport - 1)}px)`,
			).matches;
		}
		return window.innerWidth < minTableViewport;
	});

	useEffect(() => {
		if (typeof window === "undefined") return;
		const mediaQuery = `(max-width: ${Math.max(0, minTableViewport - 1)}px)`;

		if (typeof window.matchMedia === "function") {
			const mql = window.matchMedia(mediaQuery);
			const handleChange = (event: MediaQueryListEvent) => {
				setIsListLayout(event.matches);
			};

			setIsListLayout(mql.matches);
			if (typeof mql.addEventListener === "function") {
				mql.addEventListener("change", handleChange);
				return () => mql.removeEventListener("change", handleChange);
			}

			mql.addListener(handleChange);
			return () => mql.removeListener(handleChange);
		}

		const handleResize = () => {
			setIsListLayout(window.innerWidth < minTableViewport);
		};
		handleResize();
		window.addEventListener("resize", handleResize);
		return () => window.removeEventListener("resize", handleResize);
	}, [minTableViewport]);

	return isListLayout;
}

export function QuotaPolicyPage() {
	const adminToken = readAdminToken();
	const { pushToast } = useToast();
	const prefs = useUiPrefs();

	const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
	const [selectedWeightTab, setSelectedWeightTab] = useState<string>("global");
	const [ratioRows, setRatioRows] = useState<RatioDraftRow[]>([]);
	const [ratioError, setRatioError] = useState<string | null>(null);
	const [isSavingRatio, setIsSavingRatio] = useState(false);
	const [failedRows, setFailedRows] = useState<SaveFailure[]>([]);
	const [lastSave, setLastSave] = useState<LastSaveState | null>(null);
	const [globalRatioRows, setGlobalRatioRows] = useState<RatioDraftRow[]>([]);
	const [globalRatioError, setGlobalRatioError] = useState<string | null>(null);
	const [isSavingGlobalRatio, setIsSavingGlobalRatio] = useState(false);
	const [globalFailedRows, setGlobalFailedRows] = useState<SaveFailure[]>([]);
	const [lastGlobalSave, setLastGlobalSave] = useState<LastSaveState | null>(
		null,
	);
	const [isUpdatingNodePolicy, setIsUpdatingNodePolicy] = useState(false);
	const [hoveredUserId, setHoveredUserId] = useState<string | null>(null);
	const ratioEditorListLayout = useRatioEditorListLayout(
		RATIO_TABLE_MIN_VIEWPORT,
	);

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const usersQuery = useQuery({
		queryKey: ["adminUsers", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminUsers(adminToken, signal),
	});

	const globalWeightRowsQuery = useQuery({
		queryKey: ["adminQuotaPolicyGlobalWeightRows", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminQuotaPolicyGlobalWeightRows(adminToken, signal),
		refetchOnWindowFocus: false,
	});

	const nodes = nodesQuery.data?.items ?? [];
	const users = usersQuery.data?.items ?? [];

	useEffect(() => {
		if (nodes.length === 0) {
			setSelectedNodeId(null);
			if (selectedWeightTab !== "global") {
				setSelectedWeightTab("global");
			}
			return;
		}

		if (selectedWeightTab === "global") {
			if (selectedNodeId !== null) {
				setSelectedNodeId(null);
			}
			return;
		}

		if (!nodes.some((node) => node.node_id === selectedWeightTab)) {
			setSelectedWeightTab("global");
			setSelectedNodeId(null);
			return;
		}

		if (selectedNodeId !== selectedWeightTab) {
			setSelectedNodeId(selectedWeightTab);
		}
	}, [nodes, selectedNodeId, selectedWeightTab]);

	const weightRowsQuery = useQuery({
		queryKey: ["adminQuotaPolicyNodeWeightRows", adminToken, selectedNodeId],
		enabled:
			adminToken.length > 0 &&
			selectedWeightTab !== "global" &&
			Boolean(selectedNodeId),
		queryFn: ({ signal }) =>
			fetchAdminQuotaPolicyNodeWeightRows(
				adminToken,
				selectedNodeId ?? "",
				signal,
			),
		refetchOnWindowFocus: false,
	});

	const nodePolicyQuery = useQuery({
		queryKey: ["adminQuotaPolicyNodePolicy", adminToken, selectedNodeId],
		enabled:
			adminToken.length > 0 &&
			selectedWeightTab !== "global" &&
			Boolean(selectedNodeId),
		queryFn: ({ signal }) =>
			fetchAdminQuotaPolicyNodePolicy(adminToken, selectedNodeId ?? "", signal),
		refetchOnWindowFocus: false,
	});

	useEffect(() => {
		if (!selectedNodeId) {
			setRatioRows([]);
			setRatioError(null);
			setFailedRows([]);
			setLastSave(null);
			return;
		}
		if (!weightRowsQuery.data) {
			return;
		}
		setRatioRows(toDraftRows(weightRowsQuery.data.items));
		setRatioError(null);
		setFailedRows([]);
	}, [selectedNodeId, weightRowsQuery.data]);

	useEffect(() => {
		if (!globalWeightRowsQuery.data) {
			return;
		}
		setGlobalRatioRows(toGlobalDraftRows(globalWeightRowsQuery.data.items));
		setGlobalRatioError(null);
		setGlobalFailedRows([]);
	}, [globalWeightRowsQuery.data]);

	const selectedNode = useMemo(() => {
		if (!selectedNodeId) return null;
		return nodes.find((node) => node.node_id === selectedNodeId) ?? null;
	}, [nodes, selectedNodeId]);

	const handleSelectWeightTab = (tabId: string) => {
		setSelectedWeightTab(tabId);
		setHoveredUserId(null);
		if (tabId === "global") {
			setSelectedNodeId(null);
			setRatioError(null);
			setFailedRows([]);
			setLastSave(null);
			return;
		}
		setSelectedNodeId(tabId);
		setRatioError(null);
		setFailedRows([]);
		setLastSave(null);
	};

	const globalWeightByUserId = useMemo(() => {
		const map = new Map<string, number>();
		for (const row of globalWeightRowsQuery.data?.items ?? []) {
			map.set(row.user_id, row.editor_weight);
		}
		return map;
	}, [globalWeightRowsQuery.data]);

	const nodeInheritGlobal = nodePolicyQuery.data?.inherit_global ?? true;
	const inheritedNodeBasisPoints = useMemo(
		() =>
			weightsToBasisPoints(
				ratioRows.map(
					(row) =>
						globalWeightByUserId.get(row.userId) ?? DEFAULT_GLOBAL_WEIGHT,
				),
			),
		[ratioRows, globalWeightByUserId],
	);
	const displayRatioRows = useMemo(() => {
		if (!nodeInheritGlobal) {
			return ratioRows;
		}
		return ratioRows.map((row, index) => ({
			...row,
			basisPoints: inheritedNodeBasisPoints[index] ?? 0,
			locked: false,
		}));
	}, [nodeInheritGlobal, ratioRows, inheritedNodeBasisPoints]);

	const ratioBasisPoints = useMemo(
		() => displayRatioRows.map((row) => row.basisPoints),
		[displayRatioRows],
	);
	const computedWeights = useMemo(
		() =>
			nodeInheritGlobal
				? displayRatioRows.map(
						(row) =>
							globalWeightByUserId.get(row.userId) ?? DEFAULT_GLOBAL_WEIGHT,
					)
				: basisPointsToWeights(ratioBasisPoints),
		[
			nodeInheritGlobal,
			displayRatioRows,
			globalWeightByUserId,
			ratioBasisPoints,
		],
	);
	const totalBasisPoints = useMemo(
		() => ratioBasisPoints.reduce((acc, value) => acc + value, 0),
		[ratioBasisPoints],
	);
	const unlockedCount = useMemo(
		() =>
			nodeInheritGlobal ? 0 : ratioRows.filter((row) => !row.locked).length,
		[nodeInheritGlobal, ratioRows],
	);
	const changedRows = useMemo(() => {
		if (nodeInheritGlobal) {
			return [];
		}
		return ratioRows.filter((row, index) => {
			const targetWeight = computedWeights[index] ?? 0;
			return (
				row.serverStoredWeight === null ||
				row.serverStoredWeight !== targetWeight
			);
		});
	}, [nodeInheritGlobal, ratioRows, computedWeights]);

	const globalRatioBasisPoints = useMemo(
		() => globalRatioRows.map((row) => row.basisPoints),
		[globalRatioRows],
	);
	const globalComputedWeights = useMemo(
		() => basisPointsToWeights(globalRatioBasisPoints),
		[globalRatioBasisPoints],
	);
	const globalTotalBasisPoints = useMemo(
		() => globalRatioBasisPoints.reduce((acc, value) => acc + value, 0),
		[globalRatioBasisPoints],
	);
	const globalUnlockedCount = useMemo(
		() => globalRatioRows.filter((row) => !row.locked).length,
		[globalRatioRows],
	);
	const globalChangedRows = useMemo(
		() =>
			globalRatioRows.filter((row, index) => {
				const targetWeight = globalComputedWeights[index] ?? 0;
				if (row.serverStoredWeight === null) {
					return targetWeight !== DEFAULT_GLOBAL_WEIGHT;
				}
				return row.serverStoredWeight !== targetWeight;
			}),
		[globalRatioRows, globalComputedWeights],
	);

	const pieSegments = useMemo(
		() => buildPieSegments(displayRatioRows),
		[displayRatioRows],
	);
	const pieSlices = useMemo(() => {
		let cursor = 0;
		return pieSegments.map((segment, index) => {
			const startAngle = cursor;
			const sweep = (segment.basisPoints / RATIO_BASIS_POINTS) * 360;
			const endAngle = startAngle + sweep;
			cursor = endAngle;
			return {
				...segment,
				index,
				startAngle,
				endAngle,
			};
		});
	}, [pieSegments]);
	const firstPieSegment = pieSegments[0];
	const globalPieSegments = useMemo(
		() => buildPieSegments(globalRatioRows),
		[globalRatioRows],
	);
	const globalPieSlices = useMemo(() => {
		let cursor = 0;
		return globalPieSegments.map((segment, index) => {
			const startAngle = cursor;
			const sweep = (segment.basisPoints / RATIO_BASIS_POINTS) * 360;
			const endAngle = startAngle + sweep;
			cursor = endAngle;
			return {
				...segment,
				index,
				startAngle,
				endAngle,
			};
		});
	}, [globalPieSegments]);
	const firstGlobalPieSegment = globalPieSegments[0];

	const canSaveRatio =
		!nodeInheritGlobal &&
		ratioRows.length > 0 &&
		totalBasisPoints === RATIO_BASIS_POINTS &&
		!isSavingRatio &&
		!isUpdatingNodePolicy &&
		changedRows.length > 0;

	const saveBlockedReason = (() => {
		if (ratioRows.length === 0) return "No allocatable users on this node.";
		if (nodeInheritGlobal) {
			return "This node currently inherits global defaults. Disable inherit to edit node overrides.";
		}
		if (totalBasisPoints !== RATIO_BASIS_POINTS) {
			if (unlockedCount === 0) {
				return "All rows are locked and total is not 100%. Unlock at least one row.";
			}
			return "Total ratio must be exactly 100% before saving.";
		}
		if (changedRows.length === 0) return "No pending changes.";
		return null;
	})();

	const canSaveGlobalRatio =
		globalRatioRows.length > 0 &&
		globalTotalBasisPoints === RATIO_BASIS_POINTS &&
		!isSavingGlobalRatio &&
		globalChangedRows.length > 0;

	const globalSaveBlockedReason = (() => {
		if (globalRatioRows.length === 0) return "No users available.";
		if (globalTotalBasisPoints !== RATIO_BASIS_POINTS) {
			if (globalUnlockedCount === 0) {
				return "All rows are locked and total is not 100%. Unlock at least one row.";
			}
			return "Total ratio must be exactly 100% before saving.";
		}
		if (globalChangedRows.length === 0) return "No pending changes.";
		return null;
	})();

	const applyRatioEdit = (userId: string, nextBasisPoints: number) => {
		if (nodeInheritGlobal) {
			return;
		}
		setRatioError(null);
		setFailedRows([]);
		setRatioRows((prev) => {
			const result = rebalanceAfterEdit(
				prev.map((row) => ({
					rowId: row.userId,
					basisPoints: row.basisPoints,
					locked: row.locked,
				})),
				userId,
				nextBasisPoints,
			);
			if (!result.ok) {
				setRatioError(result.reason);
				return prev;
			}
			const nextById = new Map(
				result.rows.map((row) => [row.rowId, row.basisPoints]),
			);
			return prev.map((row) => ({
				...row,
				basisPoints: nextById.get(row.userId) ?? row.basisPoints,
			}));
		});
	};

	const toggleRowLock = (userId: string) => {
		if (nodeInheritGlobal) {
			return;
		}
		setRatioRows((prev) =>
			prev.map((row) =>
				row.userId === userId ? { ...row, locked: !row.locked } : row,
			),
		);
	};

	const resetToServerValues = () => {
		if (nodeInheritGlobal) {
			return;
		}
		setRatioError(null);
		setFailedRows([]);
		setRatioRows((prev) => {
			const serverWeights = prev.map((row) => row.serverStoredWeight ?? 0);
			const resetBasis = weightsToBasisPoints(serverWeights);
			return prev.map((row, index) => ({
				...row,
				basisPoints: resetBasis[index] ?? 0,
				locked: false,
			}));
		});
	};

	const persistRatioRows = async (onlyUserIds?: Set<string>) => {
		if (nodeInheritGlobal || !selectedNodeId || ratioRows.length === 0) {
			return;
		}

		const targetByUserId = new Map<string, number>();
		for (let index = 0; index < ratioRows.length; index += 1) {
			const row = ratioRows[index];
			if (!row) {
				continue;
			}
			targetByUserId.set(row.userId, computedWeights[index] ?? 0);
		}

		const candidates = ratioRows.filter((row) => {
			if (onlyUserIds && !onlyUserIds.has(row.userId)) {
				return false;
			}
			const targetWeight = targetByUserId.get(row.userId) ?? 0;
			return (
				row.serverStoredWeight === null ||
				row.serverStoredWeight !== targetWeight
			);
		});
		if (candidates.length === 0) {
			pushToast({
				variant: "success",
				message: "No ratio changes to save.",
			});
			return;
		}

		setIsSavingRatio(true);
		setRatioError(null);
		const failures: SaveFailure[] = [];
		const successByUserId = new Map<string, number>();

		for (const row of candidates) {
			const targetWeight = targetByUserId.get(row.userId) ?? 0;
			try {
				await putAdminUserNodeWeight(
					adminToken,
					row.userId,
					selectedNodeId,
					targetWeight,
				);
				successByUserId.set(row.userId, targetWeight);
			} catch (err) {
				failures.push({
					userId: row.userId,
					displayName: row.displayName,
					targetWeight,
					error: formatError(err),
				});
			}
		}

		if (successByUserId.size > 0) {
			setRatioRows((prev) =>
				prev.map((row) => {
					const target = successByUserId.get(row.userId);
					if (target === undefined) {
						return row;
					}
					return {
						...row,
						serverStoredWeight: target,
						source: "explicit",
					};
				}),
			);
		}

		const now = new Date().toISOString();
		if (failures.length > 0) {
			setFailedRows(failures);
			setRatioError(
				`Failed to save ${failures.length} user(s). You can retry failed items only.`,
			);
			setLastSave({
				status: "partial",
				message: `Saved ${successByUserId.size}, failed ${failures.length}.`,
				at: now,
			});
			pushToast({
				variant: "error",
				message: `Saved ${successByUserId.size}, failed ${failures.length}.`,
			});
		} else {
			setFailedRows([]);
			setLastSave({
				status: "success",
				message: `Saved ${successByUserId.size} row(s).`,
				at: now,
			});
			pushToast({
				variant: "success",
				message: `Saved ${successByUserId.size} row(s).`,
			});
			await weightRowsQuery.refetch();
		}
		setIsSavingRatio(false);
	};

	const retryFailedRows = async () => {
		if (failedRows.length === 0) {
			return;
		}
		await persistRatioRows(new Set(failedRows.map((item) => item.userId)));
	};

	const applyGlobalRatioEdit = (userId: string, nextBasisPoints: number) => {
		setGlobalRatioError(null);
		setGlobalFailedRows([]);
		setGlobalRatioRows((prev) => {
			const result = rebalanceAfterEdit(
				prev.map((row) => ({
					rowId: row.userId,
					basisPoints: row.basisPoints,
					locked: row.locked,
				})),
				userId,
				nextBasisPoints,
			);
			if (!result.ok) {
				setGlobalRatioError(result.reason);
				return prev;
			}
			const nextById = new Map(
				result.rows.map((row) => [row.rowId, row.basisPoints]),
			);
			return prev.map((row) => ({
				...row,
				basisPoints: nextById.get(row.userId) ?? row.basisPoints,
			}));
		});
	};

	const toggleGlobalRowLock = (userId: string) => {
		setGlobalRatioRows((prev) =>
			prev.map((row) =>
				row.userId === userId ? { ...row, locked: !row.locked } : row,
			),
		);
	};

	const resetGlobalToServerValues = () => {
		setGlobalRatioError(null);
		setGlobalFailedRows([]);
		setGlobalRatioRows((prev) => {
			const serverWeights = prev.map(
				(row) => row.serverStoredWeight ?? DEFAULT_GLOBAL_WEIGHT,
			);
			const resetBasis = weightsToBasisPoints(serverWeights);
			return prev.map((row, index) => ({
				...row,
				basisPoints: resetBasis[index] ?? 0,
				locked: false,
			}));
		});
	};

	const persistGlobalRatioRows = async (onlyUserIds?: Set<string>) => {
		if (globalRatioRows.length === 0) {
			return;
		}

		const targetByUserId = new Map<string, number>();
		for (let index = 0; index < globalRatioRows.length; index += 1) {
			const row = globalRatioRows[index];
			if (!row) {
				continue;
			}
			targetByUserId.set(row.userId, globalComputedWeights[index] ?? 0);
		}

		const candidates = globalRatioRows.filter((row) => {
			if (onlyUserIds && !onlyUserIds.has(row.userId)) {
				return false;
			}
			const targetWeight = targetByUserId.get(row.userId) ?? 0;
			if (row.serverStoredWeight === null) {
				return targetWeight !== DEFAULT_GLOBAL_WEIGHT;
			}
			return row.serverStoredWeight !== targetWeight;
		});
		if (candidates.length === 0) {
			pushToast({
				variant: "success",
				message: "No global ratio changes to save.",
			});
			return;
		}

		setIsSavingGlobalRatio(true);
		setGlobalRatioError(null);
		const failures: SaveFailure[] = [];
		const successByUserId = new Map<string, number>();

		for (const row of candidates) {
			const targetWeight = targetByUserId.get(row.userId) ?? 0;
			try {
				await putAdminQuotaPolicyGlobalWeightRow(
					adminToken,
					row.userId,
					targetWeight,
				);
				successByUserId.set(row.userId, targetWeight);
			} catch (err) {
				failures.push({
					userId: row.userId,
					displayName: row.displayName,
					targetWeight,
					error: formatError(err),
				});
			}
		}

		if (successByUserId.size > 0) {
			setGlobalRatioRows((prev) =>
				prev.map((row) => {
					const target = successByUserId.get(row.userId);
					if (target === undefined) {
						return row;
					}
					return {
						...row,
						serverStoredWeight: target,
						source: "explicit",
					};
				}),
			);
		}

		const now = new Date().toISOString();
		if (failures.length > 0) {
			setGlobalFailedRows(failures);
			setGlobalRatioError(
				`Failed to save ${failures.length} user(s). You can retry failed items only.`,
			);
			setLastGlobalSave({
				status: "partial",
				message: `Saved ${successByUserId.size}, failed ${failures.length}.`,
				at: now,
			});
			pushToast({
				variant: "error",
				message: `Saved ${successByUserId.size}, failed ${failures.length}.`,
			});
		} else {
			setGlobalFailedRows([]);
			setLastGlobalSave({
				status: "success",
				message: `Saved ${successByUserId.size} row(s).`,
				at: now,
			});
			pushToast({
				variant: "success",
				message: `Saved ${successByUserId.size} row(s).`,
			});
			await globalWeightRowsQuery.refetch();
		}
		setIsSavingGlobalRatio(false);
	};

	const retryGlobalFailedRows = async () => {
		if (globalFailedRows.length === 0) {
			return;
		}
		await persistGlobalRatioRows(
			new Set(globalFailedRows.map((item) => item.userId)),
		);
	};

	const updateNodeInheritGlobal = async (inheritGlobal: boolean) => {
		if (!selectedNodeId) {
			return;
		}
		setIsUpdatingNodePolicy(true);
		try {
			await putAdminQuotaPolicyNodePolicy(
				adminToken,
				selectedNodeId,
				inheritGlobal,
			);
			await nodePolicyQuery.refetch();
			if (!inheritGlobal) {
				setRatioRows((prev) => {
					const baseWeights = prev.map(
						(row) =>
							globalWeightByUserId.get(row.userId) ?? DEFAULT_GLOBAL_WEIGHT,
					);
					const baseBasisPoints = weightsToBasisPoints(baseWeights);
					return prev.map((row, index) => ({
						...row,
						basisPoints: baseBasisPoints[index] ?? 0,
						locked: false,
					}));
				});
			}
			setRatioError(null);
			setFailedRows([]);
			setLastSave(null);
			pushToast({
				variant: "success",
				message: inheritGlobal
					? "Node now inherits global default ratios."
					: "Node override mode enabled. Draft initialized from global defaults.",
			});
		} catch (err) {
			pushToast({
				variant: "error",
				message: `Failed to update node inherit policy: ${formatError(err)}`,
			});
		} finally {
			setIsUpdatingNodePolicy(false);
		}
	};

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to manage quota policy."
				action={
					<Link to="/login" className="btn btn-primary">
						Go to login
					</Link>
				}
			/>
		);
	}

	if (
		nodesQuery.isLoading ||
		usersQuery.isLoading ||
		globalWeightRowsQuery.isLoading
	) {
		return (
			<PageState
				variant="loading"
				title="Loading quota policy"
				description="Fetching nodes, users, and global ratios."
			/>
		);
	}

	if (
		nodesQuery.isError ||
		usersQuery.isError ||
		globalWeightRowsQuery.isError
	) {
		const message = nodesQuery.isError
			? formatError(nodesQuery.error)
			: usersQuery.isError
				? formatError(usersQuery.error)
				: globalWeightRowsQuery.isError
					? formatError(globalWeightRowsQuery.error)
					: "Unknown error";
		return (
			<PageState
				variant="error"
				title="Failed to load quota policy"
				description={message}
				action={
					<Button
						variant="secondary"
						onClick={() => {
							nodesQuery.refetch();
							usersQuery.refetch();
							globalWeightRowsQuery.refetch();
						}}
					>
						Retry
					</Button>
				}
			/>
		);
	}

	const ratioStatusClass =
		totalBasisPoints === RATIO_BASIS_POINTS
			? "badge badge-success"
			: totalBasisPoints < RATIO_BASIS_POINTS
				? "badge badge-warning"
				: "badge badge-error";
	const globalRatioStatusClass =
		globalTotalBasisPoints === RATIO_BASIS_POINTS
			? "badge badge-success"
			: globalTotalBasisPoints < RATIO_BASIS_POINTS
				? "badge badge-warning"
				: "badge badge-error";

	return (
		<div className="space-y-6">
			<PageHeader
				title="Quota policy"
				description="Shared node quota budgets, user tiers, and node-scoped ratio weights."
			/>

			<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
				<div className="space-y-1">
					<h2 className="text-lg font-semibold">Node budgets</h2>
					<p className="text-sm opacity-70">
						Set the per-cycle quota budget on each node (0 = unlimited / shared
						quota disabled). Quota reset is edited in node details.
					</p>
				</div>

				<ResourceTable
					tableClassName="table-fixed w-full"
					headers={[
						{ key: "node", label: "Node" },
						{ key: "budget", label: "Quota budget" },
						{ key: "reset", label: "Reset" },
					]}
				>
					{nodes.map((node) => (
						<tr key={node.node_id}>
							<td className="align-top">
								<div className="flex flex-col gap-1 min-w-0">
									<Link
										to="/nodes/$nodeId"
										params={{ nodeId: node.node_id }}
										className="link link-hover font-semibold block truncate"
										title="Open node details"
									>
										{node.node_name}
									</Link>
									<div className="font-mono text-xs opacity-70 break-all">
										{node.node_id}
									</div>
								</div>
							</td>
							<td className="align-top">
								<NodeQuotaEditor
									value={node.quota_limit_bytes}
									onApply={async (nextBytes) => {
										try {
											await patchAdminNode(adminToken, node.node_id, {
												quota_limit_bytes: nextBytes,
											});
											pushToast({
												variant: "success",
												message: "Node quota budget updated.",
											});
											await nodesQuery.refetch();
										} catch (err) {
											const message = formatError(err);
											pushToast({
												variant: "error",
												message: `Failed to update node quota budget: ${message}`,
											});
											throw new Error(message);
										}
									}}
								/>
							</td>
							<td className="align-top">
								<div className="text-xs opacity-70 font-mono">
									{formatNodeQuotaResetBrief(node.quota_reset)}
								</div>
							</td>
						</tr>
					))}
				</ResourceTable>
			</div>

			<div className="rounded-box border border-base-200 bg-base-100 px-4 pt-4 md:px-6 md:pt-5">
				<div className="space-y-2">
					<div className="text-sm font-medium opacity-80">分配规则作用域</div>
					<div
						role="tablist"
						aria-label="Weight configuration tabs"
						className="flex flex-wrap items-end gap-2"
					>
						<button
							type="button"
							role="tab"
							aria-selected={selectedWeightTab === "global"}
							className={`-mb-px rounded-t-lg border border-base-200 border-b-0 px-3 py-2 text-sm font-medium transition-colors md:px-4 ${
								selectedWeightTab === "global"
									? "bg-base-100 text-primary border-primary/60 border-t-2 shadow-sm"
									: "bg-base-200/60 text-base-content/70 hover:bg-base-200"
							}`}
							onClick={() => handleSelectWeightTab("global")}
						>
							全局默认
						</button>
						{nodes.map((node) => (
							<button
								key={node.node_id}
								type="button"
								role="tab"
								aria-selected={selectedWeightTab === node.node_id}
								className={`-mb-px rounded-t-lg border border-base-200 border-b-0 px-3 py-2 text-sm font-medium transition-colors md:px-4 ${
									selectedWeightTab === node.node_id
										? "bg-base-100 text-primary border-primary/60 border-t-2 shadow-sm"
										: "bg-base-200/60 text-base-content/70 hover:bg-base-200"
								}`}
								onClick={() => handleSelectWeightTab(node.node_id)}
							>
								{node.node_name}
							</button>
						))}
					</div>
				</div>
				<div className="border-t border-base-200" />
			</div>

			{selectedWeightTab === "global" ? (
				<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
					<div className="space-y-1">
						<h2 className="text-lg font-semibold">全局默认权重比例</h2>
						<p className="text-sm opacity-70">
							全局默认分配规则。新节点默认继承该规则，关闭“继承全局默认比例”后可对单节点单独覆盖。
						</p>
					</div>

					{globalRatioRows.length === 0 ? (
						<div className="rounded-box border border-base-200 p-4 text-sm opacity-70">
							No users available.
						</div>
					) : (
						<>
							<div className="flex flex-wrap items-center gap-2">
								<span className={globalRatioStatusClass}>
									Total {formatRatioPercent(globalTotalBasisPoints)}
								</span>
								<span className="badge badge-outline">
									Users {globalRatioRows.length}
								</span>
								<span className="badge badge-outline">
									Unlocked {globalUnlockedCount}
								</span>
								{globalTotalBasisPoints < RATIO_BASIS_POINTS ? (
									<span className="text-sm text-warning">
										Remaining{" "}
										{formatRatioPercent(
											RATIO_BASIS_POINTS - globalTotalBasisPoints,
										)}
									</span>
								) : null}
							</div>

							<div className="rounded-box border border-base-200 p-4 space-y-4">
								<div className="grid gap-4 md:grid-cols-[320px_1fr]">
									<div className="flex items-center justify-center">
										<svg
											viewBox="0 0 220 220"
											className="w-64 h-64"
											role="img"
											aria-label="Global weight ratio pie chart"
										>
											<circle
												cx="110"
												cy="110"
												r="96"
												fill="oklch(var(--b2))"
											/>
											{globalPieSegments.length === 1 &&
											firstGlobalPieSegment &&
											firstGlobalPieSegment.basisPoints ===
												RATIO_BASIS_POINTS ? (
												<circle
													cx="110"
													cy="110"
													r="96"
													fill={firstGlobalPieSegment.color}
												/>
											) : (
												globalPieSlices.map((segment) => {
													const isActive =
														hoveredUserId !== null &&
														segment.userIds.includes(hoveredUserId);
													return (
														<g key={`${segment.key}-${segment.index}`}>
															<path
																d={describeArc(
																	110,
																	110,
																	96,
																	segment.startAngle,
																	segment.endAngle,
																)}
																fill={segment.color}
																opacity={
																	hoveredUserId === null || isActive ? 1 : 0.35
																}
																onMouseEnter={() => {
																	if (segment.userIds.length === 1) {
																		setHoveredUserId(
																			segment.userIds[0] ?? null,
																		);
																	}
																}}
																onMouseLeave={() => setHoveredUserId(null)}
															/>
														</g>
													);
												})
											)}
											<circle
												cx="110"
												cy="110"
												r="58"
												fill="oklch(var(--b1))"
											/>
											<text
												x="110"
												y="104"
												textAnchor="middle"
												className="fill-current text-xs"
											>
												{globalRatioRows.length} users
											</text>
											<text
												x="110"
												y="122"
												textAnchor="middle"
												className="fill-current text-sm font-semibold"
											>
												{formatRatioPercent(globalTotalBasisPoints)}
											</text>
										</svg>
									</div>

									<div
										data-testid="global-ratio-pie-legend"
										className="space-y-2"
									>
										{globalPieSegments.length === 0 ? (
											<div className="text-sm opacity-70">
												No non-zero slices yet.
											</div>
										) : (
											globalPieSegments.map((segment) => {
												const active =
													hoveredUserId !== null &&
													segment.userIds.includes(hoveredUserId);
												return (
													<div
														key={segment.key}
														className={`flex items-center justify-between rounded-box border border-base-200 px-3 py-2 ${
															hoveredUserId && !active ? "opacity-50" : ""
														}`}
													>
														<div className="flex items-center gap-2 min-w-0">
															<span
																className="inline-block h-3 w-3 rounded-full"
																style={{ backgroundColor: segment.color }}
															/>
															<span className="truncate text-sm">
																{segment.label}
															</span>
														</div>
														<span className="font-mono text-xs opacity-70">
															{formatRatioPercent(segment.basisPoints)}
														</span>
													</div>
												);
											})
										)}
									</div>
								</div>
							</div>

							<div className="rounded-box border border-base-200 p-4 space-y-4">
								{ratioEditorListLayout ? (
									<div
										data-testid="global-ratio-editor-list"
										className="space-y-3"
									>
										{globalRatioRows.map((row, index) => {
											const targetWeight = globalComputedWeights[index] ?? 0;
											return (
												<div
													key={row.userId}
													className="rounded-box border border-base-200 p-3 space-y-3"
												>
													<div className="flex items-start justify-between gap-2">
														<div className="min-w-0 space-y-1">
															<div className="font-semibold truncate">
																{row.displayName}
															</div>
															<div className="font-mono text-xs opacity-70 break-all">
																{row.userId}
															</div>
														</div>
														<span className="badge badge-ghost uppercase shrink-0">
															{row.priorityTier}
														</span>
													</div>

													<div className="space-y-3">
														<div className="space-y-1">
															<div className="text-xs opacity-70">Slider</div>
															<input
																type="range"
																className="range range-primary range-sm"
																min={0}
																max={100}
																step={0.1}
																value={row.basisPoints / 100}
																disabled={isSavingGlobalRatio}
																aria-label={`Global ratio slider for ${row.displayName}`}
																onChange={(event) => {
																	applyGlobalRatioEdit(
																		row.userId,
																		Math.round(
																			Number(event.target.value) * 100,
																		),
																	);
																}}
															/>
															<div className="font-mono text-xs opacity-70">
																{formatRatioPercent(row.basisPoints)}
															</div>
														</div>
														<div className="space-y-1">
															<div className="text-xs opacity-70">
																Input (%)
															</div>
															<input
																type="number"
																min={0}
																max={100}
																step={0.01}
																className={[
																	inputClass,
																	"font-mono w-full",
																].join(" ")}
																value={row.basisPoints / 100}
																disabled={isSavingGlobalRatio}
																aria-label={`Global ratio input for ${row.displayName}`}
																onChange={(event) => {
																	const parsed = parsePercentInput(
																		event.target.value,
																	);
																	if (!parsed.ok) {
																		setGlobalRatioError(parsed.error);
																		return;
																	}
																	applyGlobalRatioEdit(
																		row.userId,
																		parsed.basisPoints,
																	);
																}}
															/>
														</div>
													</div>

													<div className="flex items-center justify-between gap-2 border-t border-base-200 pt-3">
														<div className="min-w-0">
															<div className="text-xs opacity-70">
																Computed weight
															</div>
															<div className="font-mono text-sm">
																{targetWeight}
															</div>
															<div className="text-xs opacity-70">
																{row.source === "implicit_default"
																	? "implicit_default"
																	: "explicit"}
															</div>
														</div>
														<label className="label cursor-pointer justify-start gap-2 py-0">
															<input
																type="checkbox"
																className="checkbox checkbox-sm"
																checked={row.locked}
																disabled={isSavingGlobalRatio}
																onChange={() => toggleGlobalRowLock(row.userId)}
															/>
															<span className="label-text text-xs">Lock</span>
														</label>
													</div>
												</div>
											);
										})}
									</div>
								) : (
									<div className="overflow-x-auto">
										<table
											data-testid="global-ratio-editor-table"
											className="table table-fixed w-full min-w-[980px]"
										>
											<thead>
												<tr className="bg-base-200/50">
													<th className="w-[31%]">User</th>
													<th className="w-[10%]">Tier</th>
													<th className="w-[22%]">Slider</th>
													<th className="w-[16%]">Input (%)</th>
													<th className="w-[12%] whitespace-nowrap">
														Computed weight
													</th>
													<th className="w-[9%] whitespace-nowrap">Lock</th>
												</tr>
											</thead>
											<tbody>
												{globalRatioRows.map((row, index) => {
													const targetWeight =
														globalComputedWeights[index] ?? 0;
													return (
														<tr key={row.userId}>
															<td className="align-top">
																<div className="flex flex-col gap-1 min-w-0">
																	<span className="font-semibold truncate">
																		{row.displayName}
																	</span>
																	<span className="font-mono text-xs opacity-70 break-all">
																		{row.userId}
																	</span>
																</div>
															</td>
															<td className="align-top">
																<span className="badge badge-ghost uppercase">
																	{row.priorityTier}
																</span>
															</td>
															<td className="align-top">
																<input
																	type="range"
																	className="range range-primary range-sm"
																	min={0}
																	max={100}
																	step={0.1}
																	value={row.basisPoints / 100}
																	disabled={isSavingGlobalRatio}
																	aria-label={`Global ratio slider for ${row.displayName}`}
																	onChange={(event) => {
																		applyGlobalRatioEdit(
																			row.userId,
																			Math.round(
																				Number(event.target.value) * 100,
																			),
																		);
																	}}
																/>
																<div className="font-mono text-xs opacity-70 mt-1">
																	{formatRatioPercent(row.basisPoints)}
																</div>
															</td>
															<td className="align-top">
																<input
																	type="number"
																	min={0}
																	max={100}
																	step={0.01}
																	className={[
																		inputClass,
																		"font-mono w-full",
																	].join(" ")}
																	value={row.basisPoints / 100}
																	disabled={isSavingGlobalRatio}
																	aria-label={`Global ratio input for ${row.displayName}`}
																	onChange={(event) => {
																		const parsed = parsePercentInput(
																			event.target.value,
																		);
																		if (!parsed.ok) {
																			setGlobalRatioError(parsed.error);
																			return;
																		}
																		applyGlobalRatioEdit(
																			row.userId,
																			parsed.basisPoints,
																		);
																	}}
																/>
															</td>
															<td className="align-top whitespace-nowrap">
																<div className="font-mono text-sm">
																	{targetWeight}
																</div>
																<div className="text-xs opacity-70 whitespace-normal break-words">
																	{row.source === "implicit_default"
																		? "implicit_default"
																		: "explicit"}
																</div>
															</td>
															<td className="align-top whitespace-nowrap">
																<label className="label cursor-pointer justify-start gap-2 py-0">
																	<input
																		type="checkbox"
																		className="checkbox checkbox-sm"
																		checked={row.locked}
																		disabled={isSavingGlobalRatio}
																		onChange={() =>
																			toggleGlobalRowLock(row.userId)
																		}
																	/>
																	<span className="label-text text-xs">
																		Lock
																	</span>
																</label>
															</td>
														</tr>
													);
												})}
											</tbody>
										</table>
									</div>
								)}

								{globalRatioError ? (
									<p className="text-sm text-error">{globalRatioError}</p>
								) : null}
								{globalFailedRows.length > 0 ? (
									<div className="rounded-box border border-error/40 bg-error/5 p-3 space-y-2">
										<p className="text-sm font-medium text-error">
											Failed rows ({globalFailedRows.length})
										</p>
										<ul className="text-xs space-y-1">
											{globalFailedRows.map((row) => (
												<li key={row.userId} className="font-mono">
													{row.displayName} ({row.userId}) → {row.targetWeight}:{" "}
													{row.error}
												</li>
											))}
										</ul>
									</div>
								) : null}

								<div className="flex flex-wrap items-center gap-2">
									<Button
										variant="primary"
										loading={isSavingGlobalRatio}
										disabled={!canSaveGlobalRatio}
										onClick={() => void persistGlobalRatioRows()}
									>
										Save global ratios
									</Button>
									<Button
										variant="secondary"
										disabled={
											isSavingGlobalRatio || globalFailedRows.length === 0
										}
										onClick={() => void retryGlobalFailedRows()}
									>
										Retry failed rows
									</Button>
									<Button
										variant="ghost"
										disabled={isSavingGlobalRatio}
										onClick={resetGlobalToServerValues}
									>
										Reset to server values
									</Button>
									{globalSaveBlockedReason ? (
										<span className="text-xs opacity-70">
											{globalSaveBlockedReason}
										</span>
									) : null}
								</div>

								{lastGlobalSave ? (
									<p className="text-xs opacity-70">
										Last save ({lastGlobalSave.status}) at{" "}
										{new Date(lastGlobalSave.at).toLocaleString()}:{" "}
										{lastGlobalSave.message}
									</p>
								) : null}
							</div>
						</>
					)}
				</div>
			) : null}

			<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
				<div className="space-y-1">
					<h2 className="text-lg font-semibold">User tiers</h2>
					<p className="text-sm opacity-70">
						Tier controls pacing behavior (P1 less restrictive, P2 balanced, P3
						opportunistic).
					</p>
				</div>

				<ResourceTable
					tableClassName="table-fixed w-full"
					headers={[
						{ key: "user", label: "User" },
						{ key: "tier", label: "Tier" },
					]}
				>
					{users.map((user) => (
						<tr key={user.user_id}>
							<td className="align-top">
								<div className="flex flex-col gap-1 min-w-0">
									<Link
										to="/users/$userId"
										params={{ userId: user.user_id }}
										className="link link-hover font-semibold block truncate"
										title="Open user details"
									>
										{user.display_name}
									</Link>
									<div className="font-mono text-xs opacity-70 break-all">
										{user.user_id}
									</div>
								</div>
							</td>
							<td className="align-top">
								<select
									className={
										prefs.density === "compact"
											? "select select-bordered select-sm"
											: "select select-bordered"
									}
									value={user.priority_tier}
									onChange={async (event) => {
										const next = event.target.value as "p1" | "p2" | "p3";
										try {
											await patchAdminUser(adminToken, user.user_id, {
												priority_tier: next,
											});
											pushToast({
												variant: "success",
												message: "User tier updated.",
											});
											await usersQuery.refetch();
											await globalWeightRowsQuery.refetch();
											if (selectedNodeId) {
												await weightRowsQuery.refetch();
											}
										} catch (err) {
											const message = formatError(err);
											pushToast({
												variant: "error",
												message: `Failed to update user tier: ${message}`,
											});
										}
									}}
								>
									<option value="p1">p1</option>
									<option value="p2">p2</option>
									<option value="p3">p3</option>
								</select>
							</td>
						</tr>
					))}
				</ResourceTable>
			</div>

			{selectedWeightTab !== "global" ? (
				<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
					<div className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
						<div className="space-y-1">
							<h2 className="text-lg font-semibold">
								Node weight ratio editor
							</h2>
							<p className="text-sm opacity-70">
								Each node can inherit global defaults or use local override
								ratios. Top pie chart is display-only; bottom editor is the
								source of truth.
							</p>
						</div>
						<div className="w-full md:w-[20rem]">
							<label className="label cursor-pointer justify-start gap-3 rounded-box border border-base-200 px-3 py-2">
								<input
									type="checkbox"
									className="toggle toggle-primary"
									checked={nodeInheritGlobal}
									disabled={
										!selectedNodeId ||
										nodePolicyQuery.isLoading ||
										isUpdatingNodePolicy
									}
									onChange={(event) =>
										void updateNodeInheritGlobal(event.target.checked)
									}
								/>
								<span className="label-text text-sm">
									Inherit global default ratios
								</span>
							</label>
						</div>
					</div>

					{weightRowsQuery.isLoading || nodePolicyQuery.isLoading ? (
						<div className="text-sm opacity-70">Loading node ratio rows...</div>
					) : weightRowsQuery.isError || nodePolicyQuery.isError ? (
						<div className="space-y-3">
							<p className="text-sm text-error">
								{weightRowsQuery.isError
									? formatError(weightRowsQuery.error)
									: formatError(nodePolicyQuery.error)}
							</p>
							<Button
								variant="secondary"
								onClick={() => {
									weightRowsQuery.refetch();
									nodePolicyQuery.refetch();
								}}
							>
								Retry
							</Button>
						</div>
					) : ratioRows.length === 0 ? (
						<div className="rounded-box border border-base-200 p-4 text-sm opacity-70">
							No allocatable users for this node.
						</div>
					) : (
						<>
							<div className="rounded-box border border-base-200 p-4 space-y-4">
								<div className="flex flex-wrap items-center gap-2">
									<span className={ratioStatusClass}>
										Total {formatRatioPercent(totalBasisPoints)}
									</span>
									<span className="badge badge-outline">
										Users {displayRatioRows.length}
									</span>
									<span className="badge badge-outline">
										Unlocked {unlockedCount}
									</span>
									{selectedNode ? (
										<span className="badge badge-outline">
											{selectedNode.node_name}
										</span>
									) : null}
									<span className="badge badge-outline">
										{nodeInheritGlobal
											? "Mode inherit_global"
											: "Mode node_override"}
									</span>
									{totalBasisPoints < RATIO_BASIS_POINTS ? (
										<span className="text-sm text-warning">
											Remaining{" "}
											{formatRatioPercent(
												RATIO_BASIS_POINTS - totalBasisPoints,
											)}
										</span>
									) : null}
								</div>

								<div className="grid gap-4 md:grid-cols-[320px_1fr]">
									<div className="flex items-center justify-center">
										<svg
											viewBox="0 0 220 220"
											className="w-64 h-64"
											role="img"
											aria-label="Node weight ratio pie chart"
										>
											<circle
												cx="110"
												cy="110"
												r="96"
												fill="oklch(var(--b2))"
											/>
											{pieSegments.length === 1 &&
											firstPieSegment &&
											firstPieSegment.basisPoints === RATIO_BASIS_POINTS ? (
												<circle
													cx="110"
													cy="110"
													r="96"
													fill={firstPieSegment.color}
												/>
											) : (
												pieSlices.map((segment) => {
													const isActive =
														hoveredUserId !== null &&
														segment.userIds.includes(hoveredUserId);
													return (
														<g key={`${segment.key}-${segment.index}`}>
															<path
																d={describeArc(
																	110,
																	110,
																	96,
																	segment.startAngle,
																	segment.endAngle,
																)}
																fill={segment.color}
																opacity={
																	hoveredUserId === null || isActive ? 1 : 0.35
																}
																onMouseEnter={() => {
																	if (segment.userIds.length === 1) {
																		setHoveredUserId(
																			segment.userIds[0] ?? null,
																		);
																	}
																}}
																onMouseLeave={() => setHoveredUserId(null)}
															/>
														</g>
													);
												})
											)}
											<circle
												cx="110"
												cy="110"
												r="58"
												fill="oklch(var(--b1))"
											/>
											<text
												x="110"
												y="104"
												textAnchor="middle"
												className="fill-current text-xs"
											>
												{displayRatioRows.length} users
											</text>
											<text
												x="110"
												y="122"
												textAnchor="middle"
												className="fill-current text-sm font-semibold"
											>
												{formatRatioPercent(totalBasisPoints)}
											</text>
										</svg>
									</div>

									<div data-testid="ratio-pie-legend" className="space-y-2">
										{pieSegments.length === 0 ? (
											<div className="text-sm opacity-70">
												No non-zero slices yet.
											</div>
										) : (
											pieSegments.map((segment) => {
												const active =
													hoveredUserId !== null &&
													segment.userIds.includes(hoveredUserId);
												return (
													<div
														key={segment.key}
														className={`flex items-center justify-between rounded-box border border-base-200 px-3 py-2 ${
															hoveredUserId && !active ? "opacity-50" : ""
														}`}
													>
														<div className="flex items-center gap-2 min-w-0">
															<span
																className="inline-block h-3 w-3 rounded-full"
																style={{ backgroundColor: segment.color }}
															/>
															<span className="truncate text-sm">
																{segment.label}
															</span>
														</div>
														<span className="font-mono text-xs opacity-70">
															{formatRatioPercent(segment.basisPoints)}
														</span>
													</div>
												);
											})
										)}
									</div>
								</div>
							</div>

							<div className="rounded-box border border-base-200 p-4 space-y-4">
								{ratioEditorListLayout ? (
									<div data-testid="ratio-editor-list" className="space-y-3">
										{displayRatioRows.map((row, index) => {
											const targetWeight = computedWeights[index] ?? 0;
											const isHighlighted = hoveredUserId === row.userId;
											const rowClass = isHighlighted
												? "border-info/50 bg-info/10"
												: hoveredUserId
													? "opacity-60"
													: "";
											return (
												<div
													key={row.userId}
													className={`rounded-box border border-base-200 p-3 space-y-3 ${rowClass}`}
													onMouseEnter={() => setHoveredUserId(row.userId)}
													onMouseLeave={() => setHoveredUserId(null)}
												>
													<div className="flex items-start justify-between gap-2">
														<div className="min-w-0 space-y-1">
															<div className="font-semibold truncate">
																{row.displayName}
															</div>
															<div className="font-mono text-xs opacity-70 break-all">
																{row.userId}
															</div>
															<div className="text-xs opacity-70">
																Endpoints {row.endpointIds.length}
															</div>
														</div>
														<span className="badge badge-ghost uppercase shrink-0">
															{row.priorityTier}
														</span>
													</div>

													<div className="space-y-3">
														<div className="space-y-1">
															<div className="text-xs opacity-70">Slider</div>
															<input
																type="range"
																className="range range-primary range-sm"
																min={0}
																max={100}
																step={0.1}
																value={row.basisPoints / 100}
																disabled={
																	isSavingRatio ||
																	nodeInheritGlobal ||
																	isUpdatingNodePolicy
																}
																aria-label={`Ratio slider for ${row.displayName}`}
																onFocus={() => setHoveredUserId(row.userId)}
																onBlur={() => setHoveredUserId(null)}
																onChange={(event) => {
																	applyRatioEdit(
																		row.userId,
																		Math.round(
																			Number(event.target.value) * 100,
																		),
																	);
																}}
															/>
															<div className="font-mono text-xs opacity-70">
																{formatRatioPercent(row.basisPoints)}
															</div>
														</div>
														<div className="space-y-1">
															<div className="text-xs opacity-70">
																Input (%)
															</div>
															<input
																type="number"
																min={0}
																max={100}
																step={0.01}
																className={[
																	inputClass,
																	"font-mono w-full",
																].join(" ")}
																value={row.basisPoints / 100}
																disabled={
																	isSavingRatio ||
																	nodeInheritGlobal ||
																	isUpdatingNodePolicy
																}
																aria-label={`Ratio input for ${row.displayName}`}
																onFocus={() => setHoveredUserId(row.userId)}
																onBlur={() => setHoveredUserId(null)}
																onChange={(event) => {
																	const parsed = parsePercentInput(
																		event.target.value,
																	);
																	if (!parsed.ok) {
																		setRatioError(parsed.error);
																		return;
																	}
																	applyRatioEdit(
																		row.userId,
																		parsed.basisPoints,
																	);
																}}
															/>
														</div>
													</div>

													<div className="flex items-center justify-between gap-2 border-t border-base-200 pt-3">
														<div className="min-w-0">
															<div className="text-xs opacity-70">
																Computed weight
															</div>
															<div className="font-mono text-sm">
																{targetWeight}
															</div>
															<div className="text-xs opacity-70">
																{nodeInheritGlobal
																	? "inherited_global"
																	: row.source === "implicit_zero"
																		? "implicit_zero"
																		: "explicit"}
															</div>
														</div>
														<label className="label cursor-pointer justify-start gap-2 py-0">
															<input
																type="checkbox"
																className="checkbox checkbox-sm"
																checked={row.locked}
																disabled={
																	isSavingRatio ||
																	nodeInheritGlobal ||
																	isUpdatingNodePolicy
																}
																onChange={() => toggleRowLock(row.userId)}
															/>
															<span className="label-text text-xs">Lock</span>
														</label>
													</div>
												</div>
											);
										})}
									</div>
								) : (
									<div className="overflow-x-auto">
										<table
											data-testid="ratio-editor-table"
											className="table table-fixed w-full min-w-[980px]"
										>
											<thead>
												<tr className="bg-base-200/50">
													<th className="w-[29%]">User</th>
													<th className="w-[10%]">Tier</th>
													<th className="w-[24%]">Slider</th>
													<th className="w-[16%]">Input (%)</th>
													<th className="w-[12%] whitespace-nowrap">
														Computed weight
													</th>
													<th className="w-[9%] whitespace-nowrap">Lock</th>
												</tr>
											</thead>
											<tbody>
												{displayRatioRows.map((row, index) => {
													const targetWeight = computedWeights[index] ?? 0;
													const isHighlighted = hoveredUserId === row.userId;
													const rowClass = isHighlighted
														? "bg-info/10"
														: hoveredUserId
															? "opacity-60"
															: "";
													return (
														<tr
															key={row.userId}
															className={rowClass}
															onMouseEnter={() => setHoveredUserId(row.userId)}
															onMouseLeave={() => setHoveredUserId(null)}
														>
															<td className="align-top">
																<div className="flex flex-col gap-1 min-w-0">
																	<span className="font-semibold truncate">
																		{row.displayName}
																	</span>
																	<span className="font-mono text-xs opacity-70 break-all">
																		{row.userId}
																	</span>
																	<span className="text-xs opacity-70">
																		Endpoints {row.endpointIds.length}
																	</span>
																</div>
															</td>
															<td className="align-top">
																<span className="badge badge-ghost uppercase">
																	{row.priorityTier}
																</span>
															</td>
															<td className="align-top">
																<input
																	type="range"
																	className="range range-primary range-sm"
																	min={0}
																	max={100}
																	step={0.1}
																	value={row.basisPoints / 100}
																	disabled={
																		isSavingRatio ||
																		nodeInheritGlobal ||
																		isUpdatingNodePolicy
																	}
																	aria-label={`Ratio slider for ${row.displayName}`}
																	onFocus={() => setHoveredUserId(row.userId)}
																	onBlur={() => setHoveredUserId(null)}
																	onChange={(event) => {
																		applyRatioEdit(
																			row.userId,
																			Math.round(
																				Number(event.target.value) * 100,
																			),
																		);
																	}}
																/>
																<div className="font-mono text-xs opacity-70 mt-1">
																	{formatRatioPercent(row.basisPoints)}
																</div>
															</td>
															<td className="align-top">
																<input
																	type="number"
																	min={0}
																	max={100}
																	step={0.01}
																	className={[
																		inputClass,
																		"font-mono w-full",
																	].join(" ")}
																	value={row.basisPoints / 100}
																	disabled={
																		isSavingRatio ||
																		nodeInheritGlobal ||
																		isUpdatingNodePolicy
																	}
																	aria-label={`Ratio input for ${row.displayName}`}
																	onFocus={() => setHoveredUserId(row.userId)}
																	onBlur={() => setHoveredUserId(null)}
																	onChange={(event) => {
																		const parsed = parsePercentInput(
																			event.target.value,
																		);
																		if (!parsed.ok) {
																			setRatioError(parsed.error);
																			return;
																		}
																		applyRatioEdit(
																			row.userId,
																			parsed.basisPoints,
																		);
																	}}
																/>
															</td>
															<td className="align-top whitespace-nowrap">
																<div className="font-mono text-sm">
																	{targetWeight}
																</div>
																<div className="text-xs opacity-70 whitespace-normal break-words">
																	{nodeInheritGlobal
																		? "inherited_global"
																		: row.source === "implicit_zero"
																			? "implicit_zero"
																			: "explicit"}
																</div>
															</td>
															<td className="align-top whitespace-nowrap">
																<label className="label cursor-pointer justify-start gap-2 py-0">
																	<input
																		type="checkbox"
																		className="checkbox checkbox-sm"
																		checked={row.locked}
																		disabled={
																			isSavingRatio ||
																			nodeInheritGlobal ||
																			isUpdatingNodePolicy
																		}
																		onChange={() => toggleRowLock(row.userId)}
																	/>
																	<span className="label-text text-xs">
																		Lock
																	</span>
																</label>
															</td>
														</tr>
													);
												})}
											</tbody>
										</table>
									</div>
								)}

								{ratioError ? (
									<p className="text-sm text-error">{ratioError}</p>
								) : null}
								{failedRows.length > 0 ? (
									<div className="rounded-box border border-error/40 bg-error/5 p-3 space-y-2">
										<p className="text-sm font-medium text-error">
											Failed rows ({failedRows.length})
										</p>
										<ul className="text-xs space-y-1">
											{failedRows.map((row) => (
												<li key={row.userId} className="font-mono">
													{row.displayName} ({row.userId}) → {row.targetWeight}:{" "}
													{row.error}
												</li>
											))}
										</ul>
									</div>
								) : null}

								<div className="flex flex-wrap items-center gap-2">
									<Button
										variant="primary"
										loading={isSavingRatio}
										disabled={!canSaveRatio}
										onClick={() => void persistRatioRows()}
									>
										Save ratios
									</Button>
									<Button
										variant="secondary"
										disabled={
											isSavingRatio ||
											isUpdatingNodePolicy ||
											failedRows.length === 0
										}
										onClick={() => void retryFailedRows()}
									>
										Retry failed rows
									</Button>
									<Button
										variant="ghost"
										disabled={
											isSavingRatio || nodeInheritGlobal || isUpdatingNodePolicy
										}
										onClick={resetToServerValues}
									>
										Reset to server values
									</Button>
									{saveBlockedReason ? (
										<span className="text-xs opacity-70">
											{saveBlockedReason}
										</span>
									) : null}
								</div>

								{lastSave ? (
									<p className="text-xs opacity-70">
										Last save ({lastSave.status}) at{" "}
										{new Date(lastSave.at).toLocaleString()}: {lastSave.message}
									</p>
								) : null}
							</div>
						</>
					)}
				</div>
			) : null}
		</div>
	);
}
