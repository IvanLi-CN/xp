import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState } from "react";

import type { AdminEndpoint } from "../api/adminEndpoints";
import { fetchAdminEndpoints } from "../api/adminEndpoints";
import {
	createAdminGrantGroup,
	deleteAdminGrantGroup,
	fetchAdminGrantGroup,
	fetchAdminGrantGroups,
	replaceAdminGrantGroup,
} from "../api/adminGrantGroups";
import { fetchAdminNodes } from "../api/adminNodes";
import {
	fetchAdminUserNodeQuotas,
	putAdminUserNodeQuota,
} from "../api/adminUserNodeQuotas";
import {
	deleteAdminUser,
	fetchAdminUser,
	patchAdminUser,
	resetAdminUserToken,
} from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";
import type { UserQuotaReset } from "../api/quotaReset";
import {
	type SubscriptionFormat,
	fetchSubscription,
} from "../api/subscription";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { CopyButton } from "../components/CopyButton";
import {
	GrantAccessMatrix,
	type GrantAccessMatrixCellState,
} from "../components/GrantAccessMatrix";
import { NodeQuotaEditor } from "../components/NodeQuotaEditor";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { SubscriptionPreviewDialog } from "../components/SubscriptionPreviewDialog";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";

const PROTOCOLS = [
	{ protocolId: "vless_reality_vision_tcp", label: "VLESS" },
	{ protocolId: "ss2022_2022_blake3_aes_128_gcm", label: "SS2022" },
] as const;

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

export function UserDetailsPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
	const { userId } = useParams({ from: "/app/users/$userId" });
	const { pushToast } = useToast();
	const prefs = useUiPrefs();

	const [tab, setTab] = useState<"user" | "nodeQuotas">("user");

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";
	const selectClass =
		prefs.density === "compact"
			? "select select-bordered select-sm"
			: "select select-bordered";
	const subscriptionSelectClass = [
		selectClass,
		"w-[180px] rounded-xl font-mono text-xs h-10 min-h-10",
	]
		.filter(Boolean)
		.join(" ");

	const userQuery = useQuery({
		queryKey: ["adminUser", adminToken, userId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminUser(adminToken, userId, signal),
	});

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const nodeQuotasQuery = useQuery({
		queryKey: ["adminUserNodeQuotas", adminToken, userId],
		enabled: adminToken.length > 0 && userId.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminUserNodeQuotas(adminToken, userId, signal),
	});

	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});

	const managedGroupName = useMemo(() => {
		// Keep in sync with src/state.rs:sanitize_group_name_fragment.
		const sanitizeGroupNameFragment = (input: string): string => {
			let out = "";
			for (const ch of input) {
				const isLower = ch >= "a" && ch <= "z";
				const isUpper = ch >= "A" && ch <= "Z";
				const isDigit = ch >= "0" && ch <= "9";
				if (isLower || isDigit || ch === "-" || ch === "_") {
					out += ch;
				} else if (isUpper || isLower || isDigit) {
					out += ch.toLowerCase();
				} else {
					out += "-";
				}
			}
			return out;
		};

		let out = `managed-${sanitizeGroupNameFragment(userId)}`;
		if (out.length > 64) out = out.slice(0, 64);
		return out;
	}, [userId]);

	const grantGroupQuery = useQuery({
		queryKey: ["adminGrantGroup", adminToken, managedGroupName],
		enabled: adminToken.length > 0 && managedGroupName.length > 0,
		queryFn: async ({ signal }) => {
			try {
				return await fetchAdminGrantGroup(adminToken, managedGroupName, signal);
			} catch (err) {
				if (isBackendApiError(err) && err.status === 404) return null;
				throw err;
			}
		},
	});

	const [displayName, setDisplayName] = useState("");
	const [resetPolicy, setResetPolicy] = useState<"monthly" | "unlimited">(
		"monthly",
	);
	const [resetDay, setResetDay] = useState(1);
	const [resetTzOffsetMinutes, setResetTzOffsetMinutes] = useState(480);

	const [saveError, setSaveError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);

	const [resetTokenOpen, setResetTokenOpen] = useState(false);
	const [isResettingToken, setIsResettingToken] = useState(false);

	const [deleteOpen, setDeleteOpen] = useState(false);
	const [isDeleting, setIsDeleting] = useState(false);

	const [subFormat, setSubFormat] = useState<SubscriptionFormat>("raw");
	const [subOpen, setSubOpen] = useState(false);
	const [subLoading, setSubLoading] = useState(false);
	const [subText, setSubText] = useState("");
	const [subError, setSubError] = useState<string | null>(null);

	const [nodeFilter, setNodeFilter] = useState("");
	const [selectedByCell, setSelectedByCell] = useState<Record<string, string>>(
		{},
	);
	const [accessError, setAccessError] = useState<string | null>(null);
	const [isApplyingAccess, setIsApplyingAccess] = useState(false);
	const [accessInitForUserId, setAccessInitForUserId] = useState<string | null>(
		null,
	);

	const user = userQuery.data;
	useEffect(() => {
		if (!user) return;
		setDisplayName(user.display_name);
		if (user.quota_reset.policy === "monthly") {
			setResetPolicy("monthly");
			setResetDay(user.quota_reset.day_of_month);
			setResetTzOffsetMinutes(user.quota_reset.tz_offset_minutes);
		} else {
			setResetPolicy("unlimited");
			setResetDay(1);
			setResetTzOffsetMinutes(user.quota_reset.tz_offset_minutes);
		}
		setSaveError(null);
	}, [user]);

	useEffect(() => {
		// Initialize matrix selection from the managed group once per userId.
		if (!userId) return;
		if (accessInitForUserId === userId) return;
		if (endpointsQuery.isLoading || grantGroupQuery.isLoading) return;
		if (endpointsQuery.isError || grantGroupQuery.isError) return;
		if (!endpointsQuery.data) return;

		const endpoints = endpointsQuery.data.items ?? [];
		const group = grantGroupQuery.data;
		const supported = new Set(PROTOCOLS.map((p) => p.protocolId));
		const next: Record<string, string> = {};

		if (group) {
			for (const m of group.members) {
				if (!m.enabled) continue;
				const ep = endpoints.find((e) => e.endpoint_id === m.endpoint_id);
				if (!ep) continue;
				if (!supported.has(ep.kind)) continue;
				next[`${ep.node_id}::${ep.kind}`] = ep.endpoint_id;
			}
		}

		setSelectedByCell(next);
		setNodeFilter("");
		setAccessError(null);
		setAccessInitForUserId(userId);
	}, [
		accessInitForUserId,
		endpointsQuery.data,
		endpointsQuery.isError,
		endpointsQuery.isLoading,
		grantGroupQuery.data,
		grantGroupQuery.isError,
		grantGroupQuery.isLoading,
		userId,
	]);

	const desiredQuotaReset: UserQuotaReset = useMemo(() => {
		return resetPolicy === "monthly"
			? {
					policy: "monthly",
					day_of_month: resetDay,
					tz_offset_minutes: resetTzOffsetMinutes,
				}
			: {
					policy: "unlimited",
					tz_offset_minutes: resetTzOffsetMinutes,
				};
	}, [resetDay, resetPolicy, resetTzOffsetMinutes]);

	const isDirty = useMemo(() => {
		if (!user) return false;
		if (displayName !== user.display_name) return true;
		return (
			JSON.stringify(desiredQuotaReset) !== JSON.stringify(user.quota_reset)
		);
	}, [desiredQuotaReset, displayName, user]);

	const subscriptionToken = user?.subscription_token ?? "";

	const loadSubscriptionPreview = useCallback(async () => {
		if (!subscriptionToken) return;
		setSubLoading(true);
		setSubError(null);
		try {
			const text = await fetchSubscription(subscriptionToken, subFormat);
			setSubText(text);
		} catch (err) {
			setSubText("");
			setSubError(formatError(err));
		} finally {
			setSubLoading(false);
		}
	}, [subFormat, subscriptionToken]);
	const subscriptionUrl = useMemo(() => {
		if (!subscriptionToken) return "";
		const path = `/api/sub/${encodeURIComponent(subscriptionToken)}`;
		if (typeof window === "undefined") {
			return `${path}?format=${encodeURIComponent(subFormat)}`;
		}
		const url = new URL(path, window.location.origin);
		url.searchParams.set("format", subFormat);
		return url.toString();
	}, [subFormat, subscriptionToken]);

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to load user details."
				action={
					<Link to="/login" className="btn btn-primary">
						Go to login
					</Link>
				}
			/>
		);
	}

	if (userQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading user"
				description="Fetching user details from the xp API."
			/>
		);
	}

	if (userQuery.isError) {
		return (
			<PageState
				variant="error"
				title="Failed to load user"
				description={formatError(userQuery.error)}
				action={
					<Button variant="secondary" onClick={() => userQuery.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	}

	if (!user) {
		return (
			<PageState
				variant="empty"
				title="User not found"
				description="The user ID does not exist."
				action={
					<Link to="/users" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>
		);
	}

	const nodeQuotas = nodeQuotasQuery.data?.items ?? [];
	const quotaByNodeId = new Map(nodeQuotas.map((q) => [q.node_id, q]));
	const nodes = nodesQuery.data?.items ?? [];

	const tabs = (
		<div className="flex items-center gap-3">
			<div className="inline-flex items-center gap-2 rounded-full border border-base-200 bg-base-200/30 p-1">
				<button
					type="button"
					onClick={() => setTab("user")}
					className={[
						"px-5 py-1.5 rounded-full text-xs font-semibold",
						tab === "user"
							? "bg-base-100 border border-primary/35 text-primary shadow-sm"
							: "bg-transparent border border-transparent text-base-content/70 hover:bg-base-200/40",
					].join(" ")}
				>
					User
				</button>
				<button
					type="button"
					onClick={() => setTab("nodeQuotas")}
					className={[
						"px-5 py-1.5 rounded-full text-xs font-semibold",
						tab === "nodeQuotas"
							? "bg-base-100 border border-primary/35 text-primary shadow-sm"
							: "bg-transparent border border-transparent text-base-content/70 hover:bg-base-200/40",
					].join(" ")}
				>
					Node quotas
				</button>
			</div>
		</div>
	);

	const nodeQuotasTab = (() => {
		if (
			nodesQuery.isLoading ||
			endpointsQuery.isLoading ||
			nodeQuotasQuery.isLoading ||
			grantGroupQuery.isLoading
		) {
			return (
				<PageState
					variant="loading"
					title="Loading node quotas"
					description="Fetching nodes, endpoints, quotas and access state."
				/>
			);
		}

		if (
			nodesQuery.isError ||
			endpointsQuery.isError ||
			nodeQuotasQuery.isError ||
			grantGroupQuery.isError
		) {
			const message = nodesQuery.isError
				? formatError(nodesQuery.error)
				: endpointsQuery.isError
					? formatError(endpointsQuery.error)
					: nodeQuotasQuery.isError
						? formatError(nodeQuotasQuery.error)
						: grantGroupQuery.isError
							? formatError(grantGroupQuery.error)
							: "Unknown error";
			return (
				<PageState
					variant="error"
					title="Failed to load node quotas"
					description={message}
					action={
						<Button
							variant="secondary"
							onClick={() => {
								nodesQuery.refetch();
								endpointsQuery.refetch();
								nodeQuotasQuery.refetch();
								grantGroupQuery.refetch();
							}}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		const endpoints = endpointsQuery.data?.items ?? [];

		if (nodes.length === 0 || endpoints.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Missing dependencies"
					description={
						nodes.length === 0
							? "Create a node before configuring access."
							: "Create an endpoint before configuring access."
					}
				/>
			);
		}

		const cellKey = (nodeId: string, protocolId: string) =>
			`${nodeId}::${protocolId}`;

		const endpointsByNodeProtocol = (() => {
			const map = new Map<string, Map<string, AdminEndpoint[]>>();
			for (const ep of endpoints) {
				const protocolId = ep.kind;
				const supported = PROTOCOLS.some((p) => p.protocolId === protocolId);
				if (!supported) continue;
				if (!map.has(ep.node_id)) map.set(ep.node_id, new Map());
				const byProtocol = map.get(ep.node_id);
				if (!byProtocol) continue;
				if (!byProtocol.has(protocolId)) byProtocol.set(protocolId, []);
				byProtocol.get(protocolId)?.push(ep);
			}
			for (const [, byProtocol] of map) {
				for (const [, list] of byProtocol) {
					list.sort((a, b) => a.port - b.port || a.tag.localeCompare(b.tag));
				}
			}
			return map;
		})();

		const visibleNodes = nodes.filter((n) => {
			const q = nodeFilter.trim().toLowerCase();
			if (!q) return true;
			return (
				n.node_name.toLowerCase().includes(q) ||
				n.node_id.toLowerCase().includes(q)
			);
		});

		const selectedEndpointIds = Object.values(selectedByCell);
		const totalSelectableCells = (() => {
			let count = 0;
			for (const n of visibleNodes) {
				for (const p of PROTOCOLS) {
					const options =
						endpointsByNodeProtocol.get(n.node_id)?.get(p.protocolId) ?? [];
					if (options.length > 0) count += 1;
				}
			}
			return count;
		})();

		const quotaForNode = (nodeId: string): number => {
			return quotaByNodeId.get(nodeId)?.quota_limit_bytes ?? 0;
		};

		const buildMembers = (args: {
			selectedEndpointIds: string[];
			quotaOverride?: { nodeId: string; quotaLimitBytes: number };
		}) => {
			return args.selectedEndpointIds.map((endpointId) => {
				const endpoint = endpoints.find((ep) => ep.endpoint_id === endpointId);
				if (!endpoint) {
					throw new Error(`endpoint not found: ${endpointId}`);
				}
				const quotaLimitBytes =
					args.quotaOverride && args.quotaOverride.nodeId === endpoint.node_id
						? args.quotaOverride.quotaLimitBytes
						: quotaForNode(endpoint.node_id);
				return {
					user_id: user.user_id,
					endpoint_id: endpointId,
					enabled: true,
					quota_limit_bytes: quotaLimitBytes,
					note: null,
				};
			});
		};

		type GrantGroupMemberPayload = {
			user_id: string;
			endpoint_id: string;
			enabled: boolean;
			quota_limit_bytes: number;
			note: string | null;
		};

		type GrantGroupPurgePlanItem = {
			groupName: string;
			originalMembers: GrantGroupMemberPayload[];
			remainingMembers: GrantGroupMemberPayload[];
			action: "delete" | "replace";
		};

		const toMemberPayload = (m: {
			user_id: string;
			endpoint_id: string;
			enabled: boolean;
			quota_limit_bytes: number;
			note: string | null;
		}): GrantGroupMemberPayload => {
			return {
				user_id: m.user_id,
				endpoint_id: m.endpoint_id,
				enabled: m.enabled,
				quota_limit_bytes: m.quota_limit_bytes,
				note: m.note ?? null,
			};
		};

		const buildPurgePlanForUserInOtherGrantGroups = async (): Promise<
			GrantGroupPurgePlanItem[]
		> => {
			const list = await fetchAdminGrantGroups(adminToken);
			const plan: GrantGroupPurgePlanItem[] = [];

			for (const g of list.items) {
				const groupName = g.group_name;
				if (groupName === managedGroupName) continue;

				let detail: Awaited<ReturnType<typeof fetchAdminGrantGroup>> | null =
					null;
				try {
					detail = await fetchAdminGrantGroup(adminToken, groupName);
				} catch (err) {
					if (isBackendApiError(err) && err.status === 404) continue;
					throw err;
				}
				if (!detail) continue;

				const hasUser = detail.members.some((m) => m.user_id === user.user_id);
				if (!hasUser) continue;

				const originalMembers = detail.members.map(toMemberPayload);
				const remainingMembers = detail.members
					.filter((m) => m.user_id !== user.user_id)
					.map(toMemberPayload);

				plan.push({
					groupName,
					originalMembers,
					remainingMembers,
					action: remainingMembers.length === 0 ? "delete" : "replace",
				});
			}

			return plan;
		};

		const rollbackPurgePlan = async (plan: GrantGroupPurgePlanItem[]) => {
			const errors: string[] = [];

			for (const item of [...plan].reverse()) {
				try {
					if (item.action === "delete") {
						await createAdminGrantGroup(adminToken, {
							group_name: item.groupName,
							members: item.originalMembers,
						});
					} else {
						await replaceAdminGrantGroup(adminToken, item.groupName, {
							members: item.originalMembers,
						});
					}
				} catch (err) {
					errors.push(formatError(err));
				}
			}

			return errors;
		};

		const cells: Record<
			string,
			Record<string, GrantAccessMatrixCellState>
		> = {};
		for (const n of visibleNodes) {
			const row: Record<string, GrantAccessMatrixCellState> = {};
			for (const p of PROTOCOLS) {
				const options =
					endpointsByNodeProtocol.get(n.node_id)?.get(p.protocolId) ?? [];
				if (options.length === 0) {
					row[p.protocolId] = { value: "disabled", reason: "No endpoint" };
					continue;
				}

				const key = cellKey(n.node_id, p.protocolId);
				const selected = selectedByCell[key];
				const selectedEp = selected
					? (options.find((ep) => ep.endpoint_id === selected) ?? null)
					: null;
				row[p.protocolId] = {
					value: selectedEp ? "on" : "off",
					meta:
						options.length > 1
							? {
									options: options.map((ep) => ({
										endpointId: ep.endpoint_id,
										tag: ep.tag,
										port: ep.port,
									})),
									selectedEndpointId: selectedEp?.endpoint_id,
									port: selectedEp?.port,
								}
							: {
									endpointId: options[0].endpoint_id,
									tag: options[0].tag,
									port: options[0].port,
								},
				};
			}
			cells[n.node_id] = row;
		}

		const onToggleCell = (nodeId: string, protocolId: string) => {
			if (isApplyingAccess) return;
			const options =
				endpointsByNodeProtocol.get(nodeId)?.get(protocolId) ?? [];
			if (options.length === 0) return;
			const key = cellKey(nodeId, protocolId);
			setSelectedByCell((prev) => {
				const next = { ...prev };
				if (next[key]) delete next[key];
				else next[key] = options[0].endpoint_id;
				return next;
			});
		};

		const onSelectCellEndpoint = (
			nodeId: string,
			protocolId: string,
			endpointId: string,
		) => {
			if (isApplyingAccess) return;
			const options =
				endpointsByNodeProtocol.get(nodeId)?.get(protocolId) ?? [];
			if (!options.some((ep) => ep.endpoint_id === endpointId)) return;
			const key = cellKey(nodeId, protocolId);
			setSelectedByCell((prev) => ({ ...prev, [key]: endpointId }));
		};

		const onToggleRow = (nodeId: string) => {
			if (isApplyingAccess) return;
			const protocolIds = PROTOCOLS.map((p) => p.protocolId);
			setSelectedByCell((prev) => {
				const hasAny = protocolIds.some((pid) =>
					Boolean(prev[cellKey(nodeId, pid)]),
				);
				const next = { ...prev };
				for (const pid of protocolIds) {
					const key = cellKey(nodeId, pid);
					const options = endpointsByNodeProtocol.get(nodeId)?.get(pid) ?? [];
					if (options.length === 0) continue;
					if (hasAny) delete next[key];
					else next[key] = options[0].endpoint_id;
				}
				return next;
			});
		};

		const onToggleColumn = (protocolId: string) => {
			if (isApplyingAccess) return;
			setSelectedByCell((prev) => {
				const hasAny = visibleNodes.some((n) =>
					Boolean(prev[cellKey(n.node_id, protocolId)]),
				);
				const next = { ...prev };
				for (const n of visibleNodes) {
					const key = cellKey(n.node_id, protocolId);
					const options =
						endpointsByNodeProtocol.get(n.node_id)?.get(protocolId) ?? [];
					if (options.length === 0) continue;
					if (hasAny) delete next[key];
					else next[key] = options[0].endpoint_id;
				}
				return next;
			});
		};

		const onToggleAll = () => {
			if (isApplyingAccess) return;
			setSelectedByCell((prev) => {
				const hasAny = Object.keys(prev).length > 0;
				if (hasAny) return {};
				const next: Record<string, string> = {};
				for (const n of visibleNodes) {
					for (const p of PROTOCOLS) {
						const key = cellKey(n.node_id, p.protocolId);
						const options =
							endpointsByNodeProtocol.get(n.node_id)?.get(p.protocolId) ?? [];
						if (options.length === 0) continue;
						next[key] = options[0].endpoint_id;
					}
				}
				return next;
			});
		};

		const applyChanges = async () => {
			setAccessError(null);
			setIsApplyingAccess(true);
			let appliedPurgePlan: GrantGroupPurgePlanItem[] = [];
			try {
				const purgePlan = await buildPurgePlanForUserInOtherGrantGroups();

				// Hard cut: this UI is authoritative. Remove this user from any existing
				// grant-groups so we can safely move everything into the per-user managed
				// group (backend enforces uniqueness of (user_id, endpoint_id) globally).
				appliedPurgePlan = [];
				for (const item of purgePlan) {
					if (item.action === "delete") {
						try {
							await deleteAdminGrantGroup(adminToken, item.groupName);
						} catch (err) {
							if (!(isBackendApiError(err) && err.status === 404)) {
								throw err;
							}
						}
					} else {
						await replaceAdminGrantGroup(adminToken, item.groupName, {
							members: item.remainingMembers,
						});
					}
					appliedPurgePlan.push(item);
				}

				try {
					if (selectedEndpointIds.length === 0) {
						try {
							await deleteAdminGrantGroup(adminToken, managedGroupName);
						} catch (err) {
							if (!(isBackendApiError(err) && err.status === 404)) {
								throw err;
							}
						}
					} else {
						const members = buildMembers({ selectedEndpointIds });

						if (grantGroupQuery.data) {
							await replaceAdminGrantGroup(adminToken, managedGroupName, {
								members,
							});
						} else {
							await createAdminGrantGroup(adminToken, {
								group_name: managedGroupName,
								members,
							});
						}
					}
				} catch (err) {
					// Best-effort rollback: avoid dropping user access if managed group
					// operation fails after we already removed membership from other groups.
					const rollbackErrors = await rollbackPurgePlan(appliedPurgePlan);
					appliedPurgePlan = [];
					const message = formatError(err);
					const rollbackSuffix =
						rollbackErrors.length > 0
							? ` (rollback errors: ${rollbackErrors.join("; ")})`
							: " (rolled back)";
					throw new Error(`${message}${rollbackSuffix}`);
				}

				await grantGroupQuery.refetch();
				pushToast({ variant: "success", message: "Node quotas updated." });
			} catch (err) {
				// If purge partially succeeded but we failed before applying the managed
				// group update, restore whatever we already changed.
				if (appliedPurgePlan.length > 0) {
					await rollbackPurgePlan(appliedPurgePlan);
				}
				const message = formatError(err);
				setAccessError(message);
				pushToast({
					variant: "error",
					message: `Failed to update node quotas: ${message}`,
				});
			} finally {
				setIsApplyingAccess(false);
			}
		};

		return (
			<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-6">
				<h2 className="text-lg font-semibold">Node quotas</h2>

				<div className="rounded-box border border-base-200 bg-base-100 p-4 space-y-4">
					<div className="flex flex-col gap-3 md:flex-row md:items-center">
						<input
							className={[inputClass, "w-full md:max-w-sm bg-base-200/30"].join(
								" ",
							)}
							placeholder="Filter nodes..."
							value={nodeFilter}
							onChange={(event) => setNodeFilter(event.target.value)}
							disabled={isApplyingAccess}
						/>

						<div className="flex items-center gap-2">
							<span className="rounded-full border border-base-200 bg-base-200/40 px-4 py-2 font-mono text-xs">
								Selected {selectedEndpointIds.length} / {totalSelectableCells}
							</span>
						</div>

						<div className="flex-1" />

						<Button
							variant="secondary"
							size="sm"
							onClick={() => setSelectedByCell({})}
							disabled={isApplyingAccess || selectedEndpointIds.length === 0}
						>
							Reset
						</Button>
						<Button
							variant="primary"
							size="sm"
							loading={isApplyingAccess}
							onClick={applyChanges}
						>
							Apply changes
						</Button>
					</div>

					{accessError ? (
						<p className="text-xs text-error">{accessError}</p>
					) : null}

					<div className="flex items-baseline gap-4">
						<span className="text-sm font-semibold">Matrix</span>
						<span className="text-xs opacity-60">
							Batch rule: if any selected, clear; else select all (no invert)
						</span>
					</div>

					<GrantAccessMatrix
						disabled={isApplyingAccess}
						nodes={visibleNodes.map((n) => ({
							nodeId: n.node_id,
							label: n.node_name,
							details: (
								<NodeQuotaEditor
									value={quotaForNode(n.node_id)}
									disabled={isApplyingAccess}
									onApply={async (nextBytes) => {
										try {
											await putAdminUserNodeQuota(
												adminToken,
												user.user_id,
												n.node_id,
												nextBytes,
												quotaByNodeId.get(n.node_id)?.quota_reset_source,
											);
											await nodeQuotasQuery.refetch();
											pushToast({
												variant: "success",
												message: "Node quota updated.",
											});

											if (!grantGroupQuery.data) return;

											// Do not implicitly apply selection changes from the matrix:
											// only update quotas for endpoints already in the managed group.
											const endpointsInGroupForNode =
												grantGroupQuery.data.members
													.map((m) => {
														const ep = endpoints.find(
															(e) => e.endpoint_id === m.endpoint_id,
														);
														return ep?.node_id ?? null;
													})
													.some((nodeId) => nodeId === n.node_id);
											if (!endpointsInGroupForNode) return;

											const members = grantGroupQuery.data.members.map((m) => {
												const ep = endpoints.find(
													(e) => e.endpoint_id === m.endpoint_id,
												);
												const quotaLimitBytes =
													ep?.node_id === n.node_id
														? nextBytes
														: m.quota_limit_bytes;
												return {
													user_id: m.user_id,
													endpoint_id: m.endpoint_id,
													enabled: m.enabled,
													quota_limit_bytes: quotaLimitBytes,
													note: m.note ?? null,
												};
											});

											try {
												await replaceAdminGrantGroup(
													adminToken,
													managedGroupName,
													{
														members,
													},
												);
												await grantGroupQuery.refetch();
											} catch (err) {
												const message = formatError(err);
												pushToast({
													variant: "error",
													message: `Quota updated, but failed to sync access quotas: ${message}`,
												});
											}
										} catch (err) {
											const message = formatError(err);
											pushToast({
												variant: "error",
												message: `Failed to update node quota: ${message}`,
											});
											throw new Error(message);
										}
									}}
								/>
							),
						}))}
						protocols={PROTOCOLS.map((p) => ({
							protocolId: p.protocolId,
							label: p.label,
						}))}
						cells={cells}
						onToggleCell={onToggleCell}
						onToggleRow={onToggleRow}
						onToggleColumn={onToggleColumn}
						onToggleAll={onToggleAll}
						onSelectCellEndpoint={onSelectCellEndpoint}
					/>

					<p className="text-xs opacity-60">
						Tip: header checkboxes can show indeterminate state, but clicking
						never inverts.
					</p>
				</div>
			</div>
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="User"
				description={
					<span className="font-mono text-xs">
						{user.user_id} — {user.display_name}
					</span>
				}
				actions={
					<div className="flex items-center gap-2">
						<Link to="/users" className="btn btn-ghost btn-sm">
							Back
						</Link>
					</div>
				}
			/>

			{tabs}

			{tab === "nodeQuotas" ? (
				nodeQuotasTab
			) : (
				<>
					<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
						<h2 className="text-lg font-semibold">Profile</h2>

						<div className="grid gap-4 md:grid-cols-2">
							<label className="form-control">
								<div className="label">
									<span className="label-text">Display name</span>
								</div>
								<input
									className={inputClass}
									value={displayName}
									onChange={(e) => setDisplayName(e.target.value)}
								/>
							</label>

							<label className="form-control">
								<div className="label">
									<span className="label-text">Quota reset policy</span>
								</div>
								<select
									className={selectClass}
									value={resetPolicy}
									onChange={(e) =>
										setResetPolicy(e.target.value as "monthly" | "unlimited")
									}
								>
									<option value="monthly">monthly</option>
									<option value="unlimited">unlimited</option>
								</select>
							</label>

							<label className="form-control">
								<div className="label">
									<span className="label-text">Reset day of month</span>
								</div>
								<input
									className={inputClass}
									type="number"
									min={1}
									max={31}
									disabled={resetPolicy !== "monthly"}
									value={resetDay}
									onChange={(e) => setResetDay(Number(e.target.value))}
								/>
							</label>

							<label className="form-control">
								<div className="label">
									<span className="label-text">tz_offset_minutes</span>
								</div>
								<input
									className={inputClass}
									type="number"
									value={resetTzOffsetMinutes}
									onChange={(e) =>
										setResetTzOffsetMinutes(Number(e.target.value))
									}
								/>
							</label>
						</div>

						{saveError ? (
							<p className="text-sm text-error">{saveError}</p>
						) : null}

						<div className="flex justify-end">
							<Button
								variant="primary"
								loading={isSaving}
								disabled={!isDirty}
								onClick={async () => {
									if (!isDirty) return;
									if (displayName.trim().length === 0) {
										setSaveError("Display name is required.");
										return;
									}
									if (
										resetPolicy === "monthly" &&
										(resetDay < 1 || resetDay > 31)
									) {
										setSaveError("Reset day must be between 1 and 31.");
										return;
									}
									setSaveError(null);
									setIsSaving(true);
									try {
										await patchAdminUser(adminToken, user.user_id, {
											display_name: displayName.trim(),
											quota_reset: desiredQuotaReset,
										});
										await userQuery.refetch();
										pushToast({ variant: "success", message: "User updated." });
									} catch (err) {
										setSaveError(formatError(err));
										pushToast({
											variant: "error",
											message: "Failed to update user.",
										});
									} finally {
										setIsSaving(false);
									}
								}}
							>
								Save changes
							</Button>
						</div>
					</div>

					<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
						<div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-start">
							<div className="space-y-1 min-w-0">
								<h2 className="text-lg font-semibold">Subscription</h2>
								<div className="text-xs opacity-70">Subscription token</div>
								<div className="font-mono text-xs break-all">
									{user.subscription_token}
								</div>
								<div className="text-xs opacity-70">No inline preview</div>
								<div className="text-xs opacity-70">
									Click Fetch to open the full preview modal (no wrap).
								</div>
							</div>

							<div className="flex flex-col gap-2 md:items-end">
								<div className="flex flex-wrap items-end gap-3 md:justify-end">
									<div className="space-y-1">
										<div className="text-xs opacity-70">Format</div>
										<select
											className={subscriptionSelectClass}
											data-testid="subscription-format"
											value={subFormat}
											onChange={(e) =>
												setSubFormat(e.target.value as SubscriptionFormat)
											}
										>
											<option value="raw">raw</option>
											<option value="clash">clash</option>
										</select>
									</div>

									<CopyButton
										text={subscriptionUrl}
										label="Copy URL"
										className="h-10 min-h-10 rounded-xl px-6"
									/>

									<Button
										variant="primary"
										data-testid="subscription-fetch"
										className="h-10 min-h-10 rounded-xl px-6"
										onClick={async () => {
											setSubOpen(true);
											await loadSubscriptionPreview();
										}}
										loading={subLoading}
									>
										Fetch
									</Button>
								</div>

								<div className="flex justify-end">
									<Button
										variant="secondary"
										onClick={() => setResetTokenOpen(true)}
										disabled={isResettingToken}
									>
										Reset token
									</Button>
								</div>
							</div>
						</div>
					</div>

					<div className="rounded-box border border-base-200 bg-base-100 p-6">
						<div className="flex flex-wrap items-center justify-between gap-3">
							<div className="space-y-1">
								<h2 className="text-lg font-semibold text-error">
									Danger zone
								</h2>
								<div className="text-sm opacity-70">
									Deleting a user removes all associated grant memberships and
									quotas.
								</div>
							</div>
							<Button
								variant="danger"
								onClick={() => setDeleteOpen(true)}
								disabled={isDeleting}
							>
								Delete user
							</Button>
						</div>
					</div>
				</>
			)}

			<SubscriptionPreviewDialog
				open={subOpen}
				onClose={() => setSubOpen(false)}
				subscriptionUrl={subscriptionUrl}
				format={subFormat}
				loading={subLoading}
				content={subText}
				error={subError}
			/>

			<ConfirmDialog
				open={resetTokenOpen}
				title="Reset subscription token?"
				description="This invalidates the old token immediately."
				onCancel={() => setResetTokenOpen(false)}
				footer={
					<div className="modal-action">
						<Button
							variant="secondary"
							onClick={() => setResetTokenOpen(false)}
							disabled={isResettingToken}
						>
							Cancel
						</Button>
						<Button
							variant="primary"
							loading={isResettingToken}
							onClick={async () => {
								setIsResettingToken(true);
								try {
									await resetAdminUserToken(adminToken, user.user_id);
									await userQuery.refetch();
									pushToast({ variant: "success", message: "Token reset." });
									setResetTokenOpen(false);
								} catch (err) {
									pushToast({ variant: "error", message: formatError(err) });
								} finally {
									setIsResettingToken(false);
								}
							}}
						>
							Reset
						</Button>
					</div>
				}
			/>

			<ConfirmDialog
				open={deleteOpen}
				title="Delete user?"
				description="Deleting a user removes all associated grant memberships and quotas."
				onCancel={() => setDeleteOpen(false)}
				footer={
					<div className="modal-action">
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
							onClick={async () => {
								setIsDeleting(true);
								try {
									await deleteAdminUser(adminToken, user.user_id);
									pushToast({ variant: "success", message: "User deleted." });
									navigate({ to: "/users" });
								} catch (err) {
									pushToast({ variant: "error", message: formatError(err) });
								} finally {
									setIsDeleting(false);
								}
							}}
						>
							Delete
						</Button>
					</div>
				}
			/>
		</div>
	);
}
