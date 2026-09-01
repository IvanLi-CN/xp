import { zodResolver } from "@hookform/resolvers/zod";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import type { AdminIpUsageWindow } from "../api/adminIpUsage";
import {
	type NodeHistorySnapshot,
	fetchAdminNodeHistory,
} from "../api/adminNodeHistory";
import {
	type AdminNodeRuntimeDetailResponse,
	type NodeRuntimeEvent,
	type NodeRuntimeHistorySlot,
	fetchAdminNodeRuntime,
	startNodeRuntimeEvents,
} from "../api/adminNodeRuntime";
import {
	type AdminNode,
	type AdminNodeDeletePreviewEndpoint,
	type AdminNodeEgressProbe,
	type AdminNodePatchRequest,
	deleteAdminNodeMihomoResourcePolicy,
	fetchAdminNode,
	fetchAdminNodeDeletePreview,
	fetchAdminNodeMihomoResourcePolicy,
	patchAdminNode,
	putAdminNodeMihomoResourcePolicy,
	refreshAdminNodeEgressProbe,
} from "../api/adminNodes";
import type { AdminTcpConnectionUsageWindow } from "../api/adminTcpConnections";
import { useApiCapability } from "../api/useApiCompatibility";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { IpUsageView } from "../components/IpUsageView";
import {
	ModuleTabsLayout,
	ModuleTabsPanel,
} from "../components/ModuleTabsLayout";
import { NodeQuotaEditor } from "../components/NodeQuotaEditor";
import { NodeRepositoryQuality } from "../components/NodeRepositoryHistoryQuality";
import { useObjectNavigationDirtySections } from "../components/ObjectNavigationGuard";
import { PageHeader } from "../components/PageHeader";
import { CapabilityUnavailableState, PageState } from "../components/PageState";
import { QueryErrorState } from "../components/QueryErrorState";
import { QueryRefreshError } from "../components/QueryRefreshError";
import { ReadStateBanner } from "../components/ReadStateBanner";
import { TagInput } from "../components/TagInput";
import { TcpConnectionUsageView } from "../components/TcpConnectionUsageView";
import { useToast } from "../components/Toast";
import { TrafficView } from "../components/TrafficView";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";
import { alertClass } from "../components/ui-helpers";
import { Badge } from "../components/ui/badge";
import {
	Form,
	FormControl,
	FormDescription,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
} from "../components/ui/form";
import { Input } from "../components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";
import { useNodeTimeWindowReports } from "../hooks/useNodeTimeWindowReports";
import { useAppRuntime } from "../offline/appRuntime";
import {
	formatSyncTimestamp,
	hasQueryData,
	latestQueryDataUpdatedAt,
	queryIsOfflineBlocked,
} from "../offline/queryReadState";
import { formatBackendError as formatErrorMessage } from "../utils/backendErrorMessage";
import { validateMihomoPrivateCidr } from "../utils/mihomoPrivateCidr";
import { formatQuotaBytesHuman } from "../utils/quota";
import { resourceListCache, syncNode } from "./adminEndpointsCache";
import {
	NodeDeleteOperationStatus,
	useNodeDeleteFlow,
} from "./nodeDetailsDeleteOperation";
import {
	isNodeQuotaDraftDirty,
	nodeQuotaDraftFromNode,
	toNodeQuotaReset,
} from "./nodeQuotaDraft";
function summaryBadgeVariant(status: string) {
	switch (status) {
		case "up":
			return "success";
		case "degraded":
			return "warning";
		case "down":
			return "destructive";
		default:
			return "ghost";
	}
}
function componentBadgeVariant(status: string) {
	switch (status) {
		case "up":
			return "success";
		case "degraded":
			return "warning";
		case "down":
			return "destructive";
		case "disabled":
			return "ghost";
		default:
			return "outline";
	}
}
function eventBadgeVariant(kind: NodeRuntimeEvent["kind"]) {
	switch (kind) {
		case "status_changed":
			return "warning";
		case "restart_requested":
			return "info";
		case "restart_succeeded":
			return "success";
		case "restart_failed":
			return "destructive";
		default:
			return "ghost";
	}
}

function historySlotClass(status: string): string {
	switch (status) {
		case "up":
			return "bg-success";
		case "degraded":
			return "bg-warning";
		case "down":
			return "bg-destructive";
		case "unknown":
			return "bg-info";
		default:
			return "bg-muted";
	}
}

const SLOTS_PER_DAY = 48;
const ACTIVITY_DAYS = 7;

const quotaResetSchema = z
	.object({
		resetPolicy: z.enum(["monthly", "unlimited"]),
		resetDay: z.coerce
			.number()
			.int("Reset day must be an integer between 1 and 31."),
		resetTzOffsetMinutes: z
			.string()
			.trim()
			.refine((value) => value === "" || /^-?\d+$/.test(value), {
				message: "tz_offset_minutes must be an integer (or empty).",
			}),
	})
	.superRefine((values, ctx) => {
		if (
			values.resetPolicy === "monthly" &&
			(values.resetDay < 1 || values.resetDay > 31)
		) {
			ctx.addIssue({
				code: z.ZodIssueCode.custom,
				path: ["resetDay"],
				message: "Reset day must be an integer between 1 and 31.",
			});
		}
	});

type QuotaResetFormValues = z.infer<typeof quotaResetSchema>;
type QuotaResetFormInput = z.input<typeof quotaResetSchema>;

type RuntimeActivityRow = {
	key: string;
	label: string;
	sortKey: number;
	slots: Array<NodeRuntimeHistorySlot | null>;
};

type NodeDetailsTab =
	| "runtime"
	| "metadata"
	| "mihomo"
	| "quota"
	| "traffic"
	| "ipUsage"
	| "tcpConnections"
	| "danger";

const NODE_DETAILS_TAB_OPTIONS: Array<{
	value: NodeDetailsTab;
	label: string;
}> = [
	{ value: "runtime", label: "Service runtime" },
	{ value: "metadata", label: "Node metadata" },
	{ value: "mihomo", label: "Mihomo resources" },
	{ value: "quota", label: "Quota reset" },
	{ value: "traffic", label: "Traffic" },
	{ value: "ipUsage", label: "IP usage" },
	{ value: "tcpConnections", label: "TCP connections" },
	{ value: "danger", label: "Danger zone" },
];

function buildRuntimeActivityRows(
	recentSlots: NodeRuntimeHistorySlot[],
): RuntimeActivityRow[] {
	const byDay = new Map<string, RuntimeActivityRow>();

	for (const slot of recentSlots) {
		const at = new Date(slot.slot_start);
		if (Number.isNaN(at.getTime())) continue;

		const dayStart = new Date(at.getFullYear(), at.getMonth(), at.getDate());
		const month = String(dayStart.getMonth() + 1).padStart(2, "0");
		const day = String(dayStart.getDate()).padStart(2, "0");
		const key = `${dayStart.getFullYear()}-${month}-${day}`;

		let row = byDay.get(key);
		if (!row) {
			row = {
				key,
				sortKey: dayStart.getTime(),
				label: dayStart.toLocaleDateString(undefined, {
					month: "numeric",
					day: "numeric",
					weekday: "short",
				}),
				slots: new Array(SLOTS_PER_DAY).fill(null),
			};
			byDay.set(key, row);
		}

		const slotIndex = at.getHours() * 2 + (at.getMinutes() >= 30 ? 1 : 0);
		if (slotIndex >= 0 && slotIndex < SLOTS_PER_DAY) {
			row.slots[slotIndex] = slot;
		}
	}

	const rows = Array.from(byDay.values()).sort((a, b) => a.sortKey - b.sortKey);
	if (rows.length > 0) {
		return rows.slice(-ACTIVITY_DAYS);
	}

	const fallbackRows: RuntimeActivityRow[] = [];
	const fallbackSlots = recentSlots.slice(-SLOTS_PER_DAY * ACTIVITY_DAYS);
	for (let dayIndex = 0; dayIndex < ACTIVITY_DAYS; dayIndex += 1) {
		const sliceStart = dayIndex * SLOTS_PER_DAY;
		const daySlots = fallbackSlots.slice(
			sliceStart,
			sliceStart + SLOTS_PER_DAY,
		);
		if (daySlots.length === 0) continue;
		fallbackRows.push({
			key: `fallback-${dayIndex}`,
			label: `day ${dayIndex + 1}`,
			sortKey: dayIndex,
			slots: [
				...daySlots,
				...new Array(Math.max(0, SLOTS_PER_DAY - daySlots.length)).fill(null),
			],
		});
	}

	return fallbackRows;
}

function formatTime(value: string | null | undefined): string {
	if (!value) return "-";
	const dt = new Date(value);
	if (Number.isNaN(dt.getTime())) return value;
	return dt.toLocaleString();
}

function latestItems<T extends { date: string }>(
	items: T[],
	limit: number,
): T[] {
	return [...items]
		.sort((a, b) => b.date.localeCompare(a.date))
		.slice(0, limit);
}

function NodeHistoryFallbackPanel({
	history,
	loading,
	onRefresh,
}: {
	history: NodeHistorySnapshot;
	loading: boolean;
	onRefresh: () => void;
}) {
	const traffic = latestItems(history.daily_traffic, 90);
	const componentDays = latestItems(history.daily_component_status, 90);
	return (
		<div className="space-y-4">
			<div className={alertClass("warning", "py-2")}>
				<span>
					Live node API is unavailable. Showing the last synchronized history
					mirror from {formatTime(history.last_synced_at)}.
				</span>
			</div>
			{history.last_sync_error ? (
				<div className={alertClass("info", "py-2")}>
					<span>Last sync error: {history.last_sync_error}</span>
				</div>
			) : null}
			<div className="grid gap-3 lg:grid-cols-2">
				<div className="rounded-2xl border border-border/70 bg-muted/35 p-3">
					<div className="mb-2 flex items-center justify-between gap-2">
						<p className="text-xs uppercase tracking-wide text-muted-foreground">
							Daily traffic mirror
						</p>
						<Badge variant="outline">90d retained</Badge>
					</div>
					<div className="max-h-72 overflow-auto">
						<table className="xp-table xp-table-compact">
							<thead>
								<tr>
									<th>Date</th>
									<th>Out</th>
									<th>In</th>
								</tr>
							</thead>
							<tbody>
								{traffic.length === 0 ? (
									<tr>
										<td colSpan={3} className="opacity-60">
											No mirrored traffic yet.
										</td>
									</tr>
								) : (
									traffic.map((day) => (
										<tr key={day.date}>
											<td className="font-mono text-xs">{day.date}</td>
											<td>{formatQuotaBytesHuman(day.uplink_bytes)}</td>
											<td>{formatQuotaBytesHuman(day.downlink_bytes)}</td>
										</tr>
									))
								)}
							</tbody>
						</table>
					</div>
				</div>

				<div className="rounded-2xl border border-border/70 bg-muted/35 p-3">
					<div className="mb-2 flex items-center justify-between gap-2">
						<p className="text-xs uppercase tracking-wide text-muted-foreground">
							Daily component snapshot
						</p>
						<Badge variant="outline">90d retained</Badge>
					</div>
					<div className="max-h-72 overflow-auto">
						<table className="xp-table xp-table-compact">
							<thead>
								<tr>
									<th>Date</th>
									<th>Components</th>
								</tr>
							</thead>
							<tbody>
								{componentDays.length === 0 ? (
									<tr>
										<td colSpan={2} className="opacity-60">
											No mirrored component snapshots yet.
										</td>
									</tr>
								) : (
									componentDays.map((day) => (
										<tr key={day.date}>
											<td className="font-mono text-xs">{day.date}</td>
											<td>
												<div className="flex flex-wrap gap-1">
													{day.components.map((component) => (
														<Badge
															key={`${day.date}-${component.component}`}
															variant={componentBadgeVariant(component.status)}
															size="sm"
														>
															{component.component}: {component.status}
														</Badge>
													))}
												</div>
											</td>
										</tr>
									))
								)}
							</tbody>
						</table>
					</div>
				</div>
			</div>

			<div className="space-y-2">
				<div className="flex items-center justify-between gap-2">
					<p className="text-xs uppercase tracking-wide text-muted-foreground">
						Status change log mirror
					</p>
					<Button variant="secondary" loading={loading} onClick={onRefresh}>
						Refresh history
					</Button>
				</div>
				<div className="max-h-72 overflow-auto rounded-2xl border border-border/70 bg-muted/35">
					<table className="xp-table xp-table-compact">
						<thead>
							<tr>
								<th>Time</th>
								<th>Component</th>
								<th>Change</th>
								<th>Message</th>
							</tr>
						</thead>
						<tbody>
							{history.component_status_events.length === 0 ? (
								<tr>
									<td colSpan={4} className="opacity-60">
										No mirrored status changes in the 7-day window.
									</td>
								</tr>
							) : (
								history.component_status_events.map((event) => (
									<tr key={event.event_id}>
										<td className="font-mono text-xs">
											{formatTime(event.occurred_at)}
										</td>
										<td className="font-mono text-xs">{event.component}</td>
										<td className="font-mono text-xs">
											{event.from_status ?? "-"}
											{" -> "}
											{event.to_status ?? "-"}
										</td>
										<td className="text-xs">{event.message}</td>
									</tr>
								))
							)}
						</tbody>
					</table>
				</div>
			</div>
		</div>
	);
}

function formatSubscriptionRegion(
	value: AdminNodeEgressProbe["subscription_region"],
): string {
	switch (value) {
		case "hong_kong":
			return "HongKong";
		case "us":
			return "US";
		default:
			return value
				.split("_")
				.map((part) =>
					part.length === 0 ? part : part[0].toUpperCase() + part.slice(1),
				)
				.join(" ");
	}
}

function formatEndpointKind(
	value: AdminNodeDeletePreviewEndpoint["kind"],
): string {
	switch (value) {
		case "vless_reality_vision_tcp":
			return "VLESS Reality";
		case "ss2022_2022_blake3_aes_128_gcm":
			return "SS2022";
		default:
			return value;
	}
}

export function NodeDetailsPage() {
	const { nodeId } = useParams({ from: "/app/nodes/$nodeId" });
	const [adminToken] = useState(() => readAdminToken());
	const appRuntime = useAppRuntime();
	const prefs = useUiPrefs();
	const nodesCapability = useApiCapability("admin.nodes");
	const mihomoPrivateCidrsCapability = useApiCapability(
		"node.mihomo-resource-private-cidrs-v1",
	);
	const canReadRuntime =
		adminToken.length > 0 && nodesCapability.available && appRuntime.isOnline;
	const { pushToast } = useToast();
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const nodeQuery = useQuery({
		queryKey: ["adminNode", adminToken, nodeId],
		enabled: adminToken.length > 0 && nodesCapability.available,
		queryFn: ({ signal }) => fetchAdminNode(adminToken, nodeId, signal),
	});
	const mihomoPolicyQuery = useQuery({
		queryKey: ["adminNodeMihomoResourcePolicy", adminToken, nodeId],
		enabled:
			adminToken.length > 0 &&
			nodesCapability.available &&
			mihomoPrivateCidrsCapability.available,
		queryFn: ({ signal }) =>
			fetchAdminNodeMihomoResourcePolicy(adminToken, nodeId, signal),
	});
	const runtimeQuery = useQuery({
		queryKey: ["adminNodeRuntime", adminToken, nodeId],
		enabled: adminToken.length > 0 && nodesCapability.available,
		queryFn: ({ signal }) =>
			fetchAdminNodeRuntime(adminToken, nodeId, { eventsLimit: 200, signal }),
	});
	const historyQuery = useQuery({
		queryKey: ["adminNodeHistory", adminToken, nodeId],
		enabled: adminToken.length > 0 && nodesCapability.available,
		queryFn: ({ signal }) => fetchAdminNodeHistory(adminToken, nodeId, signal),
	});
	const [runtimeLive, setRuntimeLive] =
		useState<AdminNodeRuntimeDetailResponse | null>(null);
	const [runtimeSseConnected, setRuntimeSseConnected] = useState(false);
	const [runtimeSseError, setRuntimeSseError] = useState<string | null>(null);
	const [activeTab, setActiveTab] = useState<NodeDetailsTab>("runtime");
	const [ipUsageWindow, setIpUsageWindow] = useState<AdminIpUsageWindow>("24h");
	const [tcpConnectionsWindow, setTcpConnectionsWindow] =
		useState<AdminTcpConnectionUsageWindow>("24h");
	const {
		ipUsageDisplay,
		ipUsageQuery,
		tcpConnectionsDisplay,
		tcpConnectionsQuery,
		trafficDisplay,
		trafficQuery,
	} = useNodeTimeWindowReports({
		adminToken,
		ipUsageEnabled:
			adminToken.length > 0 &&
			nodesCapability.available &&
			activeTab === "ipUsage",
		ipUsageWindow,
		nodeId,
		tcpConnectionsEnabled:
			adminToken.length > 0 &&
			nodesCapability.available &&
			activeTab === "tcpConnections",
		tcpConnectionsWindow,
		trafficEnabled:
			adminToken.length > 0 &&
			nodesCapability.available &&
			activeTab === "traffic",
		trafficWindow: prefs.trafficWindow,
	});
	useEffect(() => {
		if (!nodeId) return;
		setRuntimeLive(null);
		setRuntimeSseError(null);
		setRuntimeSseConnected(false);
	}, [nodeId]);
	const [saveError, setSaveError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);
	const [mihomoPolicyDraft, setMihomoPolicyDraft] = useState<string[]>([]);
	const [isSavingMihomoPolicy, setIsSavingMihomoPolicy] = useState(false);
	const [isRefreshingEgressProbe, setIsRefreshingEgressProbe] = useState(false);
	const [deleteOpen, setDeleteOpen] = useState(false);
	const [isPreparingDelete, setIsPreparingDelete] = useState(false);
	const [deletePreviewEndpoints, setDeletePreviewEndpoints] = useState<
		AdminNodeDeletePreviewEndpoint[]
	>([]);
	const {
		operation: pendingDeleteOperation,
		operationId: pendingDeleteOperationId,
		isDeleting,
		submitDelete,
	} = useNodeDeleteFlow({
		adminToken,
		isOnline: appRuntime.isOnline,
		nodeId,
		deletePreviewEndpoints,
		queryClient,
		pushToast,
		navigateToNodes: () => navigate({ to: "/nodes" }),
		syncCompletedCache: () =>
			resourceListCache.nodeDeleted(
				queryClient,
				adminToken,
				nodeId,
				deletePreviewEndpoints,
			),
	});
	const quotaForm = useForm<QuotaResetFormInput, unknown, QuotaResetFormValues>(
		{
			resolver: zodResolver(quotaResetSchema),
			defaultValues: {
				resetPolicy: "monthly",
				resetDay: 1,
				resetTzOffsetMinutes: "",
			},
		},
	);
	const resetQuotaForm = quotaForm.reset;
	const resetQuotaDraft = useCallback(() => {
		if (!nodeQuery.data) return;
		resetQuotaForm(nodeQuotaDraftFromNode(nodeQuery.data));
		setSaveError(null);
	}, [nodeQuery.data, resetQuotaForm]);

	useEffect(() => {
		resetQuotaDraft();
	}, [resetQuotaDraft]);

	useEffect(() => {
		if (!nodeId) return;
		setDeleteOpen(false);
		setDeletePreviewEndpoints([]);
	}, [nodeId]);

	useEffect(() => {
		if (runtimeQuery.data) {
			setRuntimeLive(runtimeQuery.data);
		}
	}, [runtimeQuery.data]);

	useEffect(() => {
		setMihomoPolicyDraft(mihomoPolicyQuery.data?.override_cidrs ?? []);
	}, [mihomoPolicyQuery.data]);

	useEffect(() => {
		if (!canReadRuntime) return;

		let unmounted = false;
		const stream = startNodeRuntimeEvents({
			adminToken,
			nodeId,
			onOpen: () => {
				if (unmounted) return;
				setRuntimeSseConnected(true);
				setRuntimeSseError(null);
			},
			onClose: () => {
				if (unmounted) return;
				setRuntimeSseConnected(false);
			},
			onError: (error) => {
				if (unmounted) return;
				setRuntimeSseConnected(false);
				setRuntimeSseError(formatErrorMessage(error));
			},
			onMessage: (message) => {
				if (unmounted) return;
				if (message.type === "snapshot") {
					setRuntimeLive((prev) => {
						if (!prev) return prev;
						return {
							...prev,
							summary: message.data.summary,
							components: message.data.components,
							recent_slots: message.data.recent_slots,
							events: message.data.events,
						};
					});
					return;
				}
				if (message.type === "event") {
					setRuntimeLive((prev) => {
						if (!prev) return prev;
						return {
							...prev,
							events: [message.data, ...prev.events].slice(0, 200),
						};
					});
					void runtimeQuery.refetch();
					return;
				}
				if (message.type === "node_error") {
					setRuntimeSseError(message.data.error);
				}
				if (message.type === "lagged") {
					setRuntimeSseError(
						`SSE lagged: missed ${message.data.missed} events.`,
					);
					void runtimeQuery.refetch();
				}
			},
		});

		return () => {
			unmounted = true;
			stream.close();
		};
	}, [adminToken, canReadRuntime, nodeId, runtimeQuery.refetch]);

	useEffect(() => {
		if (!canReadRuntime || runtimeSseConnected) return;
		const timer = window.setInterval(() => {
			void runtimeQuery.refetch();
		}, 10000);
		return () => window.clearInterval(timer);
	}, [canReadRuntime, runtimeSseConnected, runtimeQuery.refetch]);

	const quotaValues = quotaForm.watch();

	const desiredQuotaReset = useMemo(
		() => toNodeQuotaReset(quotaValues),
		[quotaValues],
	);
	const isDirty = useMemo(
		() => isNodeQuotaDraftDirty(nodeQuery.data, quotaValues),
		[nodeQuery.data, quotaValues],
	);

	const saveQuotaReset = useCallback(async (): Promise<boolean> => {
		if (!nodeQuery.data) return false;
		const valid = await quotaForm.trigger();
		if (!valid) return false;
		if (!isDirty) {
			return true;
		}
		setIsSaving(true);
		setSaveError(null);
		const payload: AdminNodePatchRequest = {
			quota_reset: desiredQuotaReset,
		};

		try {
			await patchAdminNode(adminToken, nodeId, payload);
			syncNode(queryClient, adminToken, nodeId, payload);
			pushToast({ variant: "success", message: "Node updated." });
			await nodeQuery.refetch();
			return true;
		} catch (error) {
			const message = formatErrorMessage(error);
			setSaveError(message);
			pushToast({
				variant: "error",
				message: "Failed to update node.",
			});
			return false;
		} finally {
			setIsSaving(false);
		}
	}, [
		adminToken,
		desiredQuotaReset,
		isDirty,
		nodeId,
		nodeQuery,
		pushToast,
		quotaForm,
		queryClient,
	]);
	const handleSaveQuotaReset = quotaForm.handleSubmit(() => {
		void saveQuotaReset();
	});

	useObjectNavigationDirtySections(`node:${nodeId}`, [
		{
			id: "quota-reset",
			label: "quota reset",
			isDirty: () => isDirty,
			save: saveQuotaReset,
			discard: resetQuotaDraft,
		},
	]);

	const handleRefreshEgressProbe = async () => {
		setIsRefreshingEgressProbe(true);
		try {
			const refreshed = await refreshAdminNodeEgressProbe(adminToken, nodeId);
			if (refreshed.egress_probe) {
				queryClient.setQueryData(
					["adminNode", adminToken, nodeId],
					(previous: AdminNode | undefined) =>
						previous
							? {
									...previous,
									egress_probe: refreshed.egress_probe,
								}
							: previous,
				);
				queryClient.setQueryData(
					["adminNodes", adminToken],
					(previous: { items: AdminNode[] } | undefined) =>
						previous
							? {
									items: previous.items.map((item) =>
										item.node_id === nodeId
											? { ...item, egress_probe: refreshed.egress_probe }
											: item,
									),
								}
							: previous,
				);
			}
			pushToast({
				variant: "success",
				message: "Node egress probe refreshed.",
			});
		} catch (error) {
			const message = formatErrorMessage(error);
			pushToast({
				variant: "error",
				message: `Failed to refresh node egress probe: ${message}`,
			});
		} finally {
			setIsRefreshingEgressProbe(false);
		}
	};

	const updateMihomoPolicy = async (overrideCidrs: string[]) => {
		setIsSavingMihomoPolicy(true);
		try {
			const result = await putAdminNodeMihomoResourcePolicy(
				adminToken,
				nodeId,
				overrideCidrs,
			);
			queryClient.setQueryData(
				["adminNodeMihomoResourcePolicy", adminToken, nodeId],
				result,
			);
			setMihomoPolicyDraft(result.override_cidrs ?? []);
			pushToast({
				variant: "success",
				message: "Mihomo resource policy updated.",
			});
		} catch (error) {
			pushToast({
				variant: "error",
				message: `Failed to update Mihomo resource policy: ${formatErrorMessage(error)}`,
			});
		} finally {
			setIsSavingMihomoPolicy(false);
		}
	};

	const restoreMihomoPolicy = async () => {
		setIsSavingMihomoPolicy(true);
		try {
			const result = await deleteAdminNodeMihomoResourcePolicy(
				adminToken,
				nodeId,
			);
			queryClient.setQueryData(
				["adminNodeMihomoResourcePolicy", adminToken, nodeId],
				result,
			);
			setMihomoPolicyDraft([]);
			pushToast({
				variant: "success",
				message: "Mihomo policy restored to deployment default.",
			});
		} catch (error) {
			pushToast({
				variant: "error",
				message: `Failed to restore Mihomo policy: ${formatErrorMessage(error)}`,
			});
		} finally {
			setIsSavingMihomoPolicy(false);
		}
	};

	const handleOpenDeleteDialog = async () => {
		setIsPreparingDelete(true);
		try {
			const preview = await fetchAdminNodeDeletePreview(adminToken, nodeId);
			setDeletePreviewEndpoints(preview.endpoints);
			setDeleteOpen(true);
		} catch (error) {
			pushToast({
				variant: "error",
				message: formatErrorMessage(error),
			});
		} finally {
			setIsPreparingDelete(false);
		}
	};
	const hasPendingDeleteOperation = pendingDeleteOperationId !== null;

	const content = (() => {
		if (nodesCapability.unavailable) {
			return (
				<CapabilityUnavailableState
					title="Node details unavailable"
					reason={nodesCapability.reason}
				/>
			);
		}
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Please provide an admin token to load node details."
				/>
			);
		}

		if (nodeQuery.isLoading && !hasQueryData(nodeQuery)) {
			return (
				<PageState
					variant="loading"
					title="Loading node"
					description="Fetching node metadata."
				/>
			);
		}

		if (
			!hasQueryData(nodeQuery) &&
			queryIsOfflineBlocked(nodeQuery, appRuntime.isOnline)
		) {
			return (
				<PageState
					variant="offline"
					title="Offline cache unavailable"
					description="Open this node once while online to keep its latest runtime snapshot and history available offline."
				/>
			);
		}
		if (nodeQuery.isError && !hasQueryData(nodeQuery)) {
			return (
				<QueryErrorState
					title="Failed to load node"
					description={formatErrorMessage(nodeQuery.error)}
					error={nodeQuery.error}
					loading={nodeQuery.isFetching}
					onRetry={() => nodeQuery.refetch()}
				/>
			);
		}

		if (!nodeQuery.data) {
			return (
				<PageState
					variant="empty"
					title="Node not found"
					description="No node data is available for this ID."
				/>
			);
		}

		const runtime = runtimeLive ?? runtimeQuery.data;
		const history = historyQuery.data?.history ?? null;
		const quotaPolicy = quotaForm.watch("resetPolicy");
		const egressProbe = nodeQuery.data.egress_probe;

		return (
			<div className="space-y-4">
				<ModuleTabsLayout
					options={NODE_DETAILS_TAB_OPTIONS}
					value={activeTab}
					onValueChange={(value) => setActiveTab(value as NodeDetailsTab)}
					ariaLabel="Node details sections"
					mobileAriaLabel="Node details section"
				>
					<ModuleTabsPanel value="runtime">
						<section className="space-y-4">
							<NodeRepositoryQuality adminToken={adminToken} nodeId={nodeId} />
							<div className="flex items-center justify-between gap-3">
								<div>
									<p className="text-sm text-muted-foreground">
										Live status of xp/xray/cloudflared/ddns with 7-day history
										and key events.
									</p>
								</div>
								<div className="flex items-center gap-2">
									{runtime ? (
										<Badge
											variant={summaryBadgeVariant(runtime.summary.status)}
										>
											{runtime.summary.status}
										</Badge>
									) : null}
									<Badge variant={runtimeSseConnected ? "success" : "ghost"}>
										{runtimeSseConnected ? "live" : "polling"}
									</Badge>
								</div>
							</div>

							{runtimeQuery.isLoading && !runtime ? (
								<PageState
									variant="loading"
									title="Loading runtime"
									description="Fetching service runtime details."
								/>
							) : null}

							{!runtime &&
							!history &&
							queryIsOfflineBlocked(runtimeQuery, appRuntime.isOnline) ? (
								<PageState
									variant="offline"
									title="Offline runtime cache unavailable"
									description="Open this node's runtime tab while online to keep the latest timeline and events available offline."
								/>
							) : null}

							{runtimeQuery.isError &&
							!runtime &&
							!history &&
							!queryIsOfflineBlocked(runtimeQuery, appRuntime.isOnline) ? (
								<QueryErrorState
									title="Failed to load runtime"
									description={formatErrorMessage(runtimeQuery.error)}
									error={runtimeQuery.error}
									loading={runtimeQuery.isFetching}
									disabled={!appRuntime.isOnline}
									onRetry={() => runtimeQuery.refetch()}
								/>
							) : null}

							{runtimeQuery.isError && !runtime && history ? (
								<NodeHistoryFallbackPanel
									history={history}
									loading={historyQuery.isFetching}
									onRefresh={() => historyQuery.refetch()}
								/>
							) : null}

							{runtime ? (
								<>
									{runtimeSseError ? (
										<div className={alertClass("warning", "py-2")}>
											<span>Realtime stream degraded: {runtimeSseError}</span>
										</div>
									) : null}

									<div className="grid gap-3 lg:grid-cols-3">
										{runtime.components.map((component) => (
											<div
												key={component.component}
												className="space-y-2 rounded-2xl border border-border/70 bg-muted/35 p-3"
											>
												<div className="flex items-center justify-between gap-2">
													<p className="font-semibold">{component.component}</p>
													<Badge
														variant={componentBadgeVariant(component.status)}
														size="sm"
													>
														{component.status}
													</Badge>
												</div>
												<div className="space-y-1 font-mono text-xs opacity-80">
													<p>last_ok: {formatTime(component.last_ok_at)}</p>
													<p>last_fail: {formatTime(component.last_fail_at)}</p>
													<p>down_since: {formatTime(component.down_since)}</p>
													<p>fails: {component.consecutive_failures}</p>
													<p>recoveries: {component.recoveries_observed}</p>
													<p>restart_attempts: {component.restart_attempts}</p>
													<p>
														last_restart:{" "}
														{formatTime(component.last_restart_at)}
													</p>
													{component.last_sync_at ? (
														<p>
															last_sync: {formatTime(component.last_sync_at)}
														</p>
													) : null}
													{component.current_ipv4 ? (
														<p>ipv4: {component.current_ipv4}</p>
													) : null}
													{component.current_ipv6 ? (
														<p>ipv6: {component.current_ipv6}</p>
													) : null}
													{component.fast_mode_until ? (
														<p>
															fast_until:{" "}
															{formatTime(component.fast_mode_until)}
														</p>
													) : null}
													{component.last_error ? (
														<p className="whitespace-pre-wrap break-words text-warning">
															last_error: {component.last_error}
														</p>
													) : null}
												</div>
											</div>
										))}
									</div>

									<div>
										<div className="mb-2 flex flex-wrap items-center justify-between gap-2">
											<p className="text-xs uppercase tracking-wide text-muted-foreground">
												7-day activity (30-minute slots)
											</p>
											<div className="flex items-center gap-3 text-xs opacity-70">
												<span className="inline-flex items-center gap-1">
													<span className="size-2 rounded-[2px] bg-success" />
													up
												</span>
												<span className="inline-flex items-center gap-1">
													<span className="size-2 rounded-[2px] bg-warning" />
													degraded
												</span>
												<span className="inline-flex items-center gap-1">
													<span className="size-2 rounded-[2px] bg-destructive" />
													down
												</span>
												<span className="inline-flex items-center gap-1">
													<span className="size-2 rounded-[2px] bg-info" />
													unknown
												</span>
											</div>
										</div>

										<div className="rounded-2xl border border-border/70 bg-muted/35 p-3">
											<div className="overflow-x-auto">
												<div className="min-w-[28rem]">
													<div
														className="mb-1 grid items-center gap-2 text-xs text-muted-foreground"
														style={{
															gridTemplateColumns: "4.5rem minmax(0,1fr)",
														}}
													>
														<span />
														<div className="flex items-center justify-between">
															<span>00:00</span>
															<span>06:00</span>
															<span>12:00</span>
															<span>18:00</span>
															<span>24:00</span>
														</div>
													</div>

													<div className="space-y-1.5">
														{buildRuntimeActivityRows(runtime.recent_slots).map(
															(row) => (
																<div
																	key={row.key}
																	className="grid items-center gap-2"
																	style={{
																		gridTemplateColumns: "4.5rem minmax(0,1fr)",
																	}}
																>
																	<span className="truncate font-mono text-xs text-muted-foreground">
																		{row.label}
																	</span>
																	<div
																		className="grid h-3 min-w-0 gap-px"
																		style={{
																			gridTemplateColumns:
																				"repeat(48, minmax(0, 1fr))",
																		}}
																	>
																		{row.slots.map((slot, index) => (
																			<div
																				key={`${row.key}-${index}`}
																				className={`rounded-[1px] ${
																					slot
																						? historySlotClass(slot.status)
																						: "bg-muted/60"
																				}`}
																				title={
																					slot
																						? `${slot.slot_start} • ${slot.status}`
																						: undefined
																				}
																			/>
																		))}
																	</div>
																</div>
															),
														)}
													</div>
												</div>
											</div>
										</div>
									</div>

									<div className="space-y-2">
										<div className="flex items-center justify-between gap-2">
											<p className="text-xs uppercase tracking-wide text-muted-foreground">
												Key events
											</p>
											<Button
												variant="secondary"
												loading={runtimeQuery.isFetching}
												disabled={!appRuntime.isOnline}
												onClick={() => runtimeQuery.refetch()}
											>
												Refresh runtime
											</Button>
										</div>
										<div className="max-h-72 overflow-auto rounded-2xl border border-border/70 bg-muted/35">
											<table className="xp-table xp-table-compact">
												<thead>
													<tr>
														<th>Time</th>
														<th>Component</th>
														<th>Kind</th>
														<th>Message</th>
													</tr>
												</thead>
												<tbody>
													{runtime.events.length === 0 ? (
														<tr>
															<td colSpan={4} className="opacity-60">
																No runtime events in window.
															</td>
														</tr>
													) : (
														runtime.events.map((event) => (
															<tr key={event.event_id}>
																<td className="font-mono text-xs">
																	{formatTime(event.occurred_at)}
																</td>
																<td className="font-mono text-xs">
																	{event.component}
																</td>
																<td>
																	<Badge
																		variant={eventBadgeVariant(event.kind)}
																		size="sm"
																	>
																		{event.kind}
																	</Badge>
																</td>
																<td className="text-xs">{event.message}</td>
															</tr>
														))
													)}
												</tbody>
											</table>
										</div>
									</div>
								</>
							) : null}
						</section>
					</ModuleTabsPanel>

					<ModuleTabsPanel value="metadata">
						<section className="space-y-3">
							<div>
								<p className="text-sm text-muted-foreground">
									Read-only. Managed via xp-ops config file.
								</p>
							</div>
							<div className="rounded-2xl bg-muted/35 p-4 space-y-2">
								<div className="text-xs uppercase tracking-wide text-muted-foreground">
									Node ID
								</div>
								<div className="font-mono text-sm break-all">{nodeId}</div>
								<div className="grid gap-2 md:grid-cols-2 pt-3">
									<div>
										<div className="text-xs uppercase tracking-wide text-muted-foreground">
											Node name
										</div>
										<div className="font-mono text-sm break-all">
											{nodeQuery.data.node_name || "(empty)"}
										</div>
									</div>
									<div>
										<div className="text-xs uppercase tracking-wide text-muted-foreground">
											Access host
										</div>
										<div className="font-mono text-sm break-all">
											{nodeQuery.data.access_host || "(empty)"}
										</div>
									</div>
									<div className="md:col-span-2">
										<div className="text-xs uppercase tracking-wide text-muted-foreground">
											API base URL
										</div>
										<div className="font-mono text-sm break-all">
											{nodeQuery.data.api_base_url || "(empty)"}
										</div>
									</div>
								</div>
							</div>
							<div className="rounded-2xl border border-border/70 bg-muted/35 p-4 space-y-4">
								<div className="flex flex-wrap items-start justify-between gap-3">
									<div>
										<div className="text-xs uppercase tracking-wide text-muted-foreground">
											Node egress probe
										</div>
										<p className="text-sm text-muted-foreground">
											Subscription region is derived from the latest outbound
											public IP probe.
										</p>
									</div>
									<div className="flex items-center gap-2">
										{egressProbe ? (
											<>
												<Badge variant="outline">
													{formatSubscriptionRegion(
														egressProbe.subscription_region,
													)}
												</Badge>
												{egressProbe.stale ? (
													<Badge variant="warning">stale</Badge>
												) : (
													<Badge variant="success">fresh</Badge>
												)}
											</>
										) : (
											<Badge variant="ghost">unknown</Badge>
										)}
										<Button
											variant="secondary"
											loading={isRefreshingEgressProbe}
											disabled={appRuntime.isReadOnly}
											onClick={() => void handleRefreshEgressProbe()}
										>
											Refresh probe
										</Button>
									</div>
								</div>

								{egressProbe ? (
									<div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												Selected public IP
											</div>
											<div className="font-mono text-sm break-all">
												{egressProbe.selected_public_ip ?? "-"}
											</div>
										</div>
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												IPv4
											</div>
											<div className="font-mono text-sm break-all">
												{egressProbe.public_ipv4 ?? "-"}
											</div>
										</div>
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												IPv6
											</div>
											<div className="font-mono text-sm break-all">
												{egressProbe.public_ipv6 ?? "-"}
											</div>
										</div>
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												Country
											</div>
											<div className="font-mono text-sm break-all">
												{egressProbe.country_code ?? "-"}
											</div>
										</div>
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												Region
											</div>
											<div className="font-mono text-sm break-all">
												{egressProbe.geo_region ?? "-"}
											</div>
										</div>
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												City
											</div>
											<div className="font-mono text-sm break-all">
												{egressProbe.geo_city ?? "-"}
											</div>
										</div>
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												Operator
											</div>
											<div className="font-mono text-sm break-all">
												{egressProbe.geo_operator ?? "-"}
											</div>
										</div>
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												Checked at
											</div>
											<div className="font-mono text-sm break-all">
												{formatTime(egressProbe.checked_at)}
											</div>
										</div>
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												Last success
											</div>
											<div className="font-mono text-sm break-all">
												{formatTime(egressProbe.last_success_at)}
											</div>
										</div>
									</div>
								) : (
									<div className={alertClass("warning", "py-2")}>
										<span>
											No egress probe result is stored yet. Trigger a refresh to
											probe the node now.
										</span>
									</div>
								)}

								{egressProbe?.error_summary ? (
									<div className={alertClass("warning", "py-2")}>
										<span className="break-words">
											Last probe error: {egressProbe.error_summary}
										</span>
									</div>
								) : null}
							</div>
							<div className="text-sm text-muted-foreground">
								To change node meta, edit{" "}
								<span className="font-mono">/etc/xp/xp.env</span> and run{" "}
								<span className="font-mono">sudo xp-ops xp sync-node-meta</span>
								.
							</div>
						</section>
					</ModuleTabsPanel>

					<ModuleTabsPanel value="mihomo">
						<section className="space-y-4">
							{mihomoPrivateCidrsCapability.unavailable ? (
								<CapabilityUnavailableState
									title="Mihomo private resource policy unavailable"
									reason={mihomoPrivateCidrsCapability.reason}
								/>
							) : null}
							{!mihomoPrivateCidrsCapability.unavailable ? (
								<>
									<div className="flex flex-wrap items-start justify-between gap-3">
										<div>
											<p className="text-sm font-semibold">
												Mihomo private resource policy
											</p>
											<p className="text-sm text-muted-foreground">
												Allow only approved private CIDRs for split-DNS mirror
												resources on this node.
											</p>
										</div>
										{mihomoPolicyQuery.data ? (
											<Badge
												variant={
													mihomoPolicyQuery.data.status === "healthy"
														? "success"
														: "warning"
												}
											>
												{mihomoPolicyQuery.data.source}
											</Badge>
										) : null}
									</div>

									{mihomoPolicyQuery.isLoading ? (
										<PageState
											variant="loading"
											title="Loading Mihomo policy"
											description="Fetching node-local private CIDR policy."
										/>
									) : null}
									{mihomoPolicyQuery.isError ? (
										<QueryErrorState
											title="Mihomo policy unavailable"
											description={formatErrorMessage(mihomoPolicyQuery.error)}
											error={mihomoPolicyQuery.error}
											loading={mihomoPolicyQuery.isFetching}
											onRetry={() => mihomoPolicyQuery.refetch()}
										/>
									) : null}
									{mihomoPolicyQuery.data ? (
										<>
											<div className="grid gap-3 md:grid-cols-3">
												<div className="rounded-2xl border border-border/70 bg-muted/35 p-3">
													<div className="text-xs uppercase tracking-wide text-muted-foreground">
														Deployment default
													</div>
													<div className="mt-2 space-y-1 font-mono text-sm break-all">
														{mihomoPolicyQuery.data.deployment_default_cidrs
															.length > 0
															? mihomoPolicyQuery.data.deployment_default_cidrs.map(
																	(cidr) => <div key={cidr}>{cidr}</div>,
																)
															: "(empty)"}
													</div>
												</div>
												<div className="rounded-2xl border border-border/70 bg-muted/35 p-3">
													<div className="text-xs uppercase tracking-wide text-muted-foreground">
														Effective policy
													</div>
													<div className="mt-2 space-y-1 font-mono text-sm break-all">
														{mihomoPolicyQuery.data.effective_cidrs.length > 0
															? mihomoPolicyQuery.data.effective_cidrs.map(
																	(cidr) => <div key={cidr}>{cidr}</div>,
																)
															: "(private targets disabled)"}
													</div>
												</div>
												<div className="rounded-2xl border border-border/70 bg-muted/35 p-3">
													<div className="text-xs uppercase tracking-wide text-muted-foreground">
														Status
													</div>
													<div className="mt-2 text-sm">
														{mihomoPolicyQuery.data.status}
														{mihomoPolicyQuery.data.error ? (
															<p className="mt-1 break-words text-warning">
																{mihomoPolicyQuery.data.error}
															</p>
														) : null}
													</div>
												</div>
											</div>

											<div className="space-y-4 rounded-2xl border border-border/70 bg-muted/35 p-4">
												<TagInput
													label="Web override CIDRs"
													value={mihomoPolicyDraft}
													onChange={setMihomoPolicyDraft}
													placeholder="192.168.0.0/16"
													helperText="Add a CIDR with Enter or comma, or paste a list. Saving replaces the deployment default for this node."
													validateTag={validateMihomoPrivateCidr}
													allowPrimary={false}
													disabled={
														isSavingMihomoPolicy || appRuntime.isReadOnly
													}
												/>
												<div className="flex flex-col gap-2 border-t border-border/70 pt-4 sm:flex-row sm:items-center sm:justify-between">
													<Button
														variant="secondary"
														size="md"
														className="w-full sm:w-auto"
														loading={mihomoPolicyQuery.isFetching}
														disabled={
															!appRuntime.isOnline || isSavingMihomoPolicy
														}
														onClick={() => void mihomoPolicyQuery.refetch()}
													>
														Refresh
													</Button>
													<div className="flex flex-col gap-2 sm:flex-row sm:flex-wrap sm:justify-end">
														<Button
															variant="secondary"
															size="md"
															className="w-full sm:w-auto"
															disabled={
																isSavingMihomoPolicy || appRuntime.isReadOnly
															}
															onClick={() => void updateMihomoPolicy([])}
														>
															Disable private targets
														</Button>
														<Button
															size="md"
															className="w-full sm:w-auto"
															loading={isSavingMihomoPolicy}
															disabled={appRuntime.isReadOnly}
															onClick={() =>
																void updateMihomoPolicy(mihomoPolicyDraft)
															}
														>
															Save override
														</Button>
														<Button
															variant="secondary"
															size="md"
															className="w-full sm:w-auto"
															disabled={
																isSavingMihomoPolicy || appRuntime.isReadOnly
															}
															onClick={() => void restoreMihomoPolicy()}
														>
															Restore deployment default
														</Button>
													</div>
												</div>
											</div>
										</>
									) : null}
								</>
							) : null}
						</section>
					</ModuleTabsPanel>

					<ModuleTabsPanel value="quota">
						<section className="space-y-4">
							<div>
								<p className="text-sm text-muted-foreground">
									Runtime admin setting. Safe to edit via the admin API.
								</p>
							</div>
							<div className="rounded-2xl bg-muted/35 p-4 space-y-1">
								<div className="text-xs uppercase tracking-wide text-muted-foreground">
									Quota budget
								</div>
								<div className="text-sm text-muted-foreground">
									Total bytes per cycle for this node. Set to{" "}
									<span className="font-mono">0</span> to disable shared quota
									enforcement (unlimited).
								</div>
								<NodeQuotaEditor
									value={nodeQuery.data.quota_limit_bytes}
									disabled={isSaving || appRuntime.isReadOnly}
									onApply={async (nextBytes: number) => {
										try {
											await patchAdminNode(adminToken, nodeId, {
												quota_limit_bytes: nextBytes,
											});
											syncNode(queryClient, adminToken, nodeId, {
												quota_limit_bytes: nextBytes,
											});
											pushToast({
												variant: "success",
												message: "Node quota budget updated.",
											});
											await nodeQuery.refetch();
										} catch (error) {
											const message = formatErrorMessage(error);
											pushToast({
												variant: "error",
												message: `Failed to update node quota budget: ${message}`,
											});
											throw new Error(message);
										}
									}}
								/>
							</div>
							<Form {...quotaForm}>
								<form className="space-y-4" onSubmit={handleSaveQuotaReset}>
									<div className="grid gap-4 md:grid-cols-3">
										<FormField
											control={quotaForm.control}
											name="resetPolicy"
											render={({ field }) => (
												<FormItem>
													<FormLabel>Policy</FormLabel>
													<Select
														value={field.value}
														onValueChange={(value) => {
															field.onChange(value);
															if (value !== "monthly") {
																quotaForm.setValue("resetDay", 1, {
																	shouldDirty: true,
																	shouldValidate: false,
																});
																quotaForm.clearErrors("resetDay");
															}
														}}
													>
														<FormControl>
															<SelectTrigger>
																<SelectValue />
															</SelectTrigger>
														</FormControl>
														<SelectContent>
															<SelectItem value="monthly">monthly</SelectItem>
															<SelectItem value="unlimited">
																unlimited
															</SelectItem>
														</SelectContent>
													</Select>
													<FormMessage />
												</FormItem>
											)}
										/>
										<FormField
											control={quotaForm.control}
											name="resetDay"
											render={({ field }) => (
												<FormItem>
													<FormLabel>Day of month</FormLabel>
													<FormControl>
														<Input
															type="number"
															min={1}
															max={31}
															step={1}
															disabled={quotaPolicy !== "monthly"}
															name={field.name}
															ref={field.ref}
															onBlur={field.onBlur}
															value={
																typeof field.value === "number" ||
																typeof field.value === "string"
																	? field.value
																	: ""
															}
															onChange={(event) =>
																field.onChange(event.target.value)
															}
														/>
													</FormControl>
													<FormDescription>
														Used when the policy is monthly.
													</FormDescription>
													<FormMessage />
												</FormItem>
											)}
										/>
										<FormField
											control={quotaForm.control}
											name="resetTzOffsetMinutes"
											render={({ field }) => (
												<FormItem>
													<FormLabel>tz_offset_minutes</FormLabel>
													<FormControl>
														<Input
															{...field}
															type="text"
															placeholder="(empty)"
														/>
													</FormControl>
													<FormDescription>
														Leave empty to follow node-local defaults.
													</FormDescription>
													<FormMessage />
												</FormItem>
											)}
										/>
									</div>

									{saveError ? (
										<p className="text-sm text-destructive">{saveError}</p>
									) : null}

									<div className="flex justify-end gap-2">
										<Button
											variant="secondary"
											type="button"
											loading={nodeQuery.isFetching}
											disabled={!appRuntime.isOnline}
											onClick={() => nodeQuery.refetch()}
										>
											Refresh
										</Button>
										<Button
											type="submit"
											loading={isSaving}
											disabled={!isDirty || appRuntime.isReadOnly}
										>
											Save changes
										</Button>
									</div>
								</form>
							</Form>
						</section>
					</ModuleTabsPanel>

					<ModuleTabsPanel value="traffic">
						<div className="space-y-4">
							<NodeRepositoryQuality adminToken={adminToken} nodeId={nodeId} />
							{trafficQuery.isLoading && !trafficDisplay.data ? (
								<PageState
									variant="loading"
									title="Loading traffic"
									description="Fetching five-minute node traffic rollups."
								/>
							) : null}
							{!trafficDisplay.data &&
							queryIsOfflineBlocked(trafficQuery, appRuntime.isOnline) ? (
								<PageState
									variant="offline"
									title="Offline traffic cache unavailable"
									description="Open this tab while online to keep the latest traffic report available offline."
								/>
							) : null}
							{trafficQuery.isError &&
							!trafficDisplay.data &&
							!queryIsOfflineBlocked(trafficQuery, appRuntime.isOnline) ? (
								<QueryErrorState
									title="Failed to load traffic"
									description={formatErrorMessage(trafficQuery.error)}
									error={trafficQuery.error}
									loading={trafficQuery.isFetching}
									onRetry={() => trafficQuery.refetch()}
								/>
							) : null}
							{trafficQuery.isError && trafficDisplay.data ? (
								<QueryRefreshError
									title="Traffic refresh failed"
									description={formatErrorMessage(trafficQuery.error)}
									error={trafficQuery.error}
									loading={trafficQuery.isFetching}
									onRetry={() => trafficQuery.refetch()}
								/>
							) : null}
							{trafficDisplay.data ? (
								<TrafficView
									report={trafficDisplay.data.traffic}
									window={trafficDisplay.displayWindow ?? prefs.trafficWindow}
									onWindowChange={(next) => prefs.setTrafficWindow(next)}
									isFetching={trafficQuery.isFetching}
									isWindowPending={trafficDisplay.isWindowPending}
									showTitle={false}
								/>
							) : null}
						</div>
					</ModuleTabsPanel>

					<ModuleTabsPanel value="ipUsage">
						<div className="space-y-4">
							<NodeRepositoryQuality adminToken={adminToken} nodeId={nodeId} />
							{ipUsageQuery.isLoading && !ipUsageDisplay.data ? (
								<PageState
									variant="loading"
									title="Loading IP usage"
									description="Fetching minute-level inbound IP usage for this node."
								/>
							) : null}
							{!ipUsageDisplay.data &&
							queryIsOfflineBlocked(ipUsageQuery, appRuntime.isOnline) ? (
								<PageState
									variant="offline"
									title="Offline IP usage cache unavailable"
									description="Open this tab while online to keep the latest inbound IP report available offline."
								/>
							) : null}
							{ipUsageQuery.isError &&
							!ipUsageDisplay.data &&
							!queryIsOfflineBlocked(ipUsageQuery, appRuntime.isOnline) ? (
								<QueryErrorState
									title="Failed to load IP usage"
									description={formatErrorMessage(ipUsageQuery.error)}
									error={ipUsageQuery.error}
									loading={ipUsageQuery.isFetching}
									onRetry={() => ipUsageQuery.refetch()}
								/>
							) : null}

							{ipUsageQuery.isError && ipUsageDisplay.data ? (
								<QueryRefreshError
									title="IP usage refresh failed"
									description={formatErrorMessage(ipUsageQuery.error)}
									error={ipUsageQuery.error}
									loading={ipUsageQuery.isFetching}
									onRetry={() => ipUsageQuery.refetch()}
								/>
							) : null}

							{ipUsageDisplay.data ? (
								<IpUsageView
									title="IP usage"
									description="Per-minute unique inbound IP counts, occupancy lanes, and aggregated IP rows for this node."
									showTitle={false}
									window={ipUsageDisplay.displayWindow ?? ipUsageWindow}
									geoSource={ipUsageDisplay.data.geo_source}
									onWindowChange={setIpUsageWindow}
									report={ipUsageDisplay.data}
									isFetching={ipUsageQuery.isFetching}
									isWindowPending={ipUsageDisplay.isWindowPending}
									emptyTitle="No inbound IP activity"
								/>
							) : null}
						</div>
					</ModuleTabsPanel>

					<ModuleTabsPanel value="tcpConnections">
						<div className="space-y-4">
							<NodeRepositoryQuality adminToken={adminToken} nodeId={nodeId} />
							{tcpConnectionsQuery.isLoading && !tcpConnectionsDisplay.data ? (
								<PageState
									variant="loading"
									title="Loading TCP connection count"
									description="Fetching the node's minute-level TCP connection history."
								/>
							) : null}

							{!tcpConnectionsDisplay.data &&
							queryIsOfflineBlocked(
								tcpConnectionsQuery,
								appRuntime.isOnline,
							) ? (
								<PageState
									variant="offline"
									title="Offline TCP history cache unavailable"
									description="Open this tab while online to keep the latest TCP connection history available offline."
								/>
							) : null}

							{tcpConnectionsQuery.isError &&
							!tcpConnectionsDisplay.data &&
							!queryIsOfflineBlocked(
								tcpConnectionsQuery,
								appRuntime.isOnline,
							) ? (
								<QueryErrorState
									title="Failed to load TCP connection count"
									description={formatErrorMessage(tcpConnectionsQuery.error)}
									error={tcpConnectionsQuery.error}
									loading={tcpConnectionsQuery.isFetching}
									onRetry={() => tcpConnectionsQuery.refetch()}
								/>
							) : null}

							{tcpConnectionsQuery.isError && tcpConnectionsDisplay.data ? (
								<QueryRefreshError
									title="TCP connection refresh failed"
									description={formatErrorMessage(tcpConnectionsQuery.error)}
									error={tcpConnectionsQuery.error}
									loading={tcpConnectionsQuery.isFetching}
									onRetry={() => tcpConnectionsQuery.refetch()}
								/>
							) : null}

							{tcpConnectionsDisplay.data ? (
								<TcpConnectionUsageView
									window={
										tcpConnectionsDisplay.displayWindow ?? tcpConnectionsWindow
									}
									onWindowChange={setTcpConnectionsWindow}
									report={tcpConnectionsDisplay.data}
									isFetching={tcpConnectionsQuery.isFetching}
									isWindowPending={tcpConnectionsDisplay.isWindowPending}
									showTitle={false}
								/>
							) : null}
						</div>
					</ModuleTabsPanel>

					<ModuleTabsPanel value="danger">
						<section className="space-y-4">
							<NodeDeleteOperationStatus
								operation={pendingDeleteOperation}
								visible={hasPendingDeleteOperation}
							/>
							<p className="text-sm text-muted-foreground">
								Deleting a node removes it from the cluster membership and
								inventory. If the node still owns endpoints, the next step shows
								exactly what will be deleted.
							</p>
							<div>
								<Button
									variant="danger"
									onClick={() => void handleOpenDeleteDialog()}
									loading={isPreparingDelete}
									disabled={
										isDeleting ||
										hasPendingDeleteOperation ||
										appRuntime.isReadOnly
									}
								>
									Delete node
								</Button>
							</div>
						</section>
					</ModuleTabsPanel>
				</ModuleTabsLayout>

				<ConfirmDialog
					open={deleteOpen}
					title="Delete node?"
					description={
						deletePreviewEndpoints.length > 0
							? "This node still owns endpoints. Confirming will delete the node and the endpoints listed below."
							: "This action cannot be undone."
					}
					body={
						deletePreviewEndpoints.length > 0 ? (
							<div className="space-y-3">
								<p className="text-sm font-medium">
									Endpoints to delete: {deletePreviewEndpoints.length}
								</p>
								<div className="max-h-56 overflow-auto rounded-md border border-border">
									<table className="w-full text-left text-sm">
										<thead className="bg-muted text-xs uppercase text-muted-foreground">
											<tr>
												<th className="px-3 py-2 font-medium">Tag</th>
												<th className="px-3 py-2 font-medium">Kind</th>
												<th className="px-3 py-2 font-medium">Port</th>
											</tr>
										</thead>
										<tbody>
											{deletePreviewEndpoints.map((endpoint) => (
												<tr
													key={endpoint.endpoint_id}
													className="border-t border-border"
												>
													<td className="px-3 py-2 font-mono text-xs">
														{endpoint.tag}
													</td>
													<td className="px-3 py-2">
														{formatEndpointKind(endpoint.kind)}
													</td>
													<td className="px-3 py-2 font-mono">
														{endpoint.port}
													</td>
												</tr>
											))}
										</tbody>
									</table>
								</div>
							</div>
						) : null
					}
					onCancel={() => setDeleteOpen(false)}
					footer={
						<div className="flex justify-end gap-2">
							<Button
								variant="secondary"
								onClick={() => setDeleteOpen(false)}
								disabled={isDeleting}
							>
								Cancel
							</Button>
							<Button
								variant="danger"
								loading={isDeleting}
								onClick={() => {
									void submitDelete().finally(() => setDeleteOpen(false));
								}}
							>
								{deletePreviewEndpoints.length > 0
									? "Delete node and endpoints"
									: "Delete"}
							</Button>
						</div>
					}
				/>
			</div>
		);
	})();
	const latestSyncedAt = latestQueryDataUpdatedAt([
		nodeQuery,
		runtimeQuery,
		historyQuery,
		ipUsageQuery,
		tcpConnectionsQuery,
	]);
	const showCachedBanner =
		latestSyncedAt !== null &&
		(hasQueryData(nodeQuery) ||
			hasQueryData(runtimeQuery) ||
			hasQueryData(historyQuery)) &&
		(!appRuntime.isOnline ||
			nodeQuery.isError ||
			runtimeQuery.isError ||
			historyQuery.isError ||
			trafficQuery.isError ||
			ipUsageQuery.isError ||
			tcpConnectionsQuery.isError);
	return (
		<div className="space-y-4 sm:space-y-6">
			<PageHeader
				title="Node details"
				description="Manage node metadata and routing configuration."
				actions={
					<Button asChild variant="ghost" size="sm">
						<Link to="/nodes">Back</Link>
					</Button>
				}
			/>
			{showCachedBanner ? (
				<ReadStateBanner
					tone={!appRuntime.isOnline ? "warning" : "info"}
					variant="inline"
					dismissible
					errors={[
						nodeQuery.error,
						runtimeQuery.error,
						historyQuery.error,
						trafficQuery.error,
						ipUsageQuery.error,
						tcpConnectionsQuery.error,
					]}
					title={
						!appRuntime.isOnline
							? "Offline node snapshot"
							: "Showing cached node detail data"
					}
					description={`Last successful sync: ${formatSyncTimestamp(latestSyncedAt)}.`}
				/>
			) : null}
			{content}
		</div>
	);
}
