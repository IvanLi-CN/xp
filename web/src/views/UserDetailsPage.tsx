import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import {
	type PutAdminUserGrantItem,
	fetchAdminUserGrants,
	putAdminUserGrants,
} from "../api/adminUserGrants";
import { fetchAdminUserNodeQuotaStatus } from "../api/adminUserNodeQuotaStatus";
import { fetchAdminUserNodeQuotas } from "../api/adminUserNodeQuotas";
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
import { readAdminToken } from "../components/auth";
import { formatQuotaBytesHuman } from "../utils/quota";

const PROTOCOLS = [
	{ protocolId: "vless_reality_vision_tcp", label: "VLESS" },
	{ protocolId: "ss2022_2022_blake3_aes_128_gcm", label: "SS2022" },
] as const;

type SupportedProtocolId = (typeof PROTOCOLS)[number]["protocolId"];

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

function buildCellKey(nodeId: string, protocolId: string): string {
	return `${nodeId}::${protocolId}`;
}

export function UserDetailsPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
	const { userId } = useParams({ from: "/app/users/$userId" });
	const { pushToast } = useToast();

	const [tab, setTab] = useState<"user" | "access" | "quotaStatus">("user");
	const [displayName, setDisplayName] = useState("");
	const [resetPolicy, setResetPolicy] = useState<"monthly" | "unlimited">(
		"monthly",
	);
	const [resetDay, setResetDay] = useState(1);
	const [resetTzOffsetMinutes, setResetTzOffsetMinutes] = useState(480);
	const [isSavingUser, setIsSavingUser] = useState(false);
	const [userSaveError, setUserSaveError] = useState<string | null>(null);
	const [selectedByCell, setSelectedByCell] = useState<Record<string, string>>(
		{},
	);
	const [accessInitForUserId, setAccessInitForUserId] = useState<string | null>(
		null,
	);
	const [isApplyingAccess, setIsApplyingAccess] = useState(false);
	const [accessError, setAccessError] = useState<string | null>(null);
	const [resetTokenOpen, setResetTokenOpen] = useState(false);
	const [isResettingToken, setIsResettingToken] = useState(false);
	const [subFormat, setSubFormat] = useState<SubscriptionFormat>("raw");
	const [subOpen, setSubOpen] = useState(false);
	const [subLoading, setSubLoading] = useState(false);
	const [subText, setSubText] = useState("");
	const [subError, setSubError] = useState<string | null>(null);
	const [deleteOpen, setDeleteOpen] = useState(false);
	const [isDeleting, setIsDeleting] = useState(false);

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

	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});

	const grantsQuery = useQuery({
		queryKey: ["adminUserGrants", adminToken, userId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminUserGrants(adminToken, userId, signal),
	});

	const nodeQuotasQuery = useQuery({
		queryKey: ["adminUserNodeQuotas", adminToken, userId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminUserNodeQuotas(adminToken, userId, signal),
	});

	const nodeQuotaStatusQuery = useQuery({
		queryKey: ["adminUserNodeQuotaStatus", adminToken, userId],
		enabled: adminToken.length > 0 && tab === "quotaStatus",
		queryFn: ({ signal }) =>
			fetchAdminUserNodeQuotaStatus(adminToken, userId, signal),
	});

	const user = userQuery.data;
	const subscriptionToken = user?.subscription_token ?? "";
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
		setUserSaveError(null);
	}, [user]);

	const endpoints = endpointsQuery.data?.items ?? [];
	const grants = grantsQuery.data?.items ?? [];

	const endpointById = useMemo(() => {
		const map = new Map<string, (typeof endpoints)[number]>();
		for (const endpoint of endpoints) {
			map.set(endpoint.endpoint_id, endpoint);
		}
		return map;
	}, [endpoints]);

	const optionsByCell = useMemo(() => {
		const map = new Map<
			string,
			Array<{ endpointId: string; tag: string; port: number }>
		>();
		for (const endpoint of endpoints) {
			const key = buildCellKey(endpoint.node_id, endpoint.kind);
			const list = map.get(key) ?? [];
			list.push({
				endpointId: endpoint.endpoint_id,
				tag: endpoint.tag,
				port: endpoint.port,
			});
			list.sort((a, b) => a.port - b.port || a.tag.localeCompare(b.tag));
			map.set(key, list);
		}
		return map;
	}, [endpoints]);

	useEffect(() => {
		if (!userId || accessInitForUserId === userId) return;
		if (endpointsQuery.isLoading || grantsQuery.isLoading) return;
		if (endpointsQuery.isError || grantsQuery.isError) return;

		const next: Record<string, string> = {};
		for (const grant of grants) {
			if (!grant.enabled) continue;
			const endpoint = endpointById.get(grant.endpoint_id);
			if (!endpoint) continue;
			next[buildCellKey(endpoint.node_id, endpoint.kind)] =
				endpoint.endpoint_id;
		}
		setSelectedByCell(next);
		setAccessError(null);
		setAccessInitForUserId(userId);
	}, [
		accessInitForUserId,
		endpointById,
		endpointsQuery.isError,
		endpointsQuery.isLoading,
		grants,
		grantsQuery.isError,
		grantsQuery.isLoading,
		userId,
	]);

	const cells = useMemo(() => {
		const byNode: Record<
			string,
			Record<string, GrantAccessMatrixCellState>
		> = {};
		const nodeList = nodesQuery.data?.items ?? [];
		for (const node of nodeList) {
			const row: Record<string, GrantAccessMatrixCellState> = {};
			for (const protocol of PROTOCOLS) {
				const key = buildCellKey(node.node_id, protocol.protocolId);
				const options = optionsByCell.get(key) ?? [];
				if (options.length === 0) {
					row[protocol.protocolId] = {
						value: "disabled",
						reason: "No endpoint",
					};
					continue;
				}

				const selectedEndpointId = selectedByCell[key];
				const selected = options.find(
					(option) => option.endpointId === selectedEndpointId,
				);
				row[protocol.protocolId] = {
					value: selected ? "on" : "off",
					meta: {
						endpointId: selected?.endpointId,
						selectedEndpointId,
						tag: selected?.tag,
						port: selected?.port,
						options: options.map((option) => ({
							endpointId: option.endpointId,
							tag: option.tag,
							port: option.port,
						})),
					},
				};
			}
			byNode[node.node_id] = row;
		}
		return byNode;
	}, [nodesQuery.data?.items, optionsByCell, selectedByCell]);

	function toggleCell(nodeId: string, protocolId: SupportedProtocolId) {
		const key = buildCellKey(nodeId, protocolId);
		const options = optionsByCell.get(key) ?? [];
		if (options.length === 0) return;
		setSelectedByCell((prev) => {
			const next = { ...prev };
			if (next[key]) {
				delete next[key];
			} else {
				next[key] = options[0].endpointId;
			}
			return next;
		});
	}

	function setCellEndpoint(
		nodeId: string,
		protocolId: SupportedProtocolId,
		endpointId: string,
	) {
		const key = buildCellKey(nodeId, protocolId);
		setSelectedByCell((prev) => ({ ...prev, [key]: endpointId }));
	}

	function toggleRow(nodeId: string) {
		const keys = PROTOCOLS.map((protocol) =>
			buildCellKey(nodeId, protocol.protocolId),
		).filter((key) => (optionsByCell.get(key) ?? []).length > 0);
		if (keys.length === 0) return;
		const hasOn = keys.some((key) => Boolean(selectedByCell[key]));
		setSelectedByCell((prev) => {
			const next = { ...prev };
			for (const key of keys) {
				if (hasOn) {
					delete next[key];
				} else {
					next[key] = optionsByCell.get(key)?.[0]?.endpointId ?? next[key];
				}
			}
			return next;
		});
	}

	function toggleColumn(protocolId: SupportedProtocolId) {
		const nodeIds = nodesQuery.data?.items?.map((node) => node.node_id) ?? [];
		const keys = nodeIds
			.map((nodeId) => buildCellKey(nodeId, protocolId))
			.filter((key) => (optionsByCell.get(key) ?? []).length > 0);
		if (keys.length === 0) return;
		const hasOn = keys.some((key) => Boolean(selectedByCell[key]));
		setSelectedByCell((prev) => {
			const next = { ...prev };
			for (const key of keys) {
				if (hasOn) {
					delete next[key];
				} else {
					next[key] = optionsByCell.get(key)?.[0]?.endpointId ?? next[key];
				}
			}
			return next;
		});
	}

	function toggleAll() {
		const keys = Array.from(optionsByCell.keys()).filter(
			(key) => (optionsByCell.get(key) ?? []).length > 0,
		);
		if (keys.length === 0) return;
		const hasOn = keys.some((key) => Boolean(selectedByCell[key]));
		setSelectedByCell((prev) => {
			const next = { ...prev };
			for (const key of keys) {
				if (hasOn) {
					delete next[key];
				} else {
					next[key] = optionsByCell.get(key)?.[0]?.endpointId ?? next[key];
				}
			}
			return next;
		});
	}

	const grantByEndpoint = useMemo(() => {
		const map = new Map<string, (typeof grants)[number]>();
		for (const grant of grants) {
			map.set(grant.endpoint_id, grant);
		}
		return map;
	}, [grants]);

	const selectedEndpointIds = useMemo(() => {
		return Array.from(new Set(Object.values(selectedByCell)));
	}, [selectedByCell]);
	const isAccessDataLoading =
		nodesQuery.isLoading || endpointsQuery.isLoading || grantsQuery.isLoading;
	const accessDataError = nodesQuery.isError
		? `Nodes: ${formatError(nodesQuery.error)}`
		: endpointsQuery.isError
			? `Endpoints: ${formatError(endpointsQuery.error)}`
			: grantsQuery.isError
				? `Grants: ${formatError(grantsQuery.error)}`
				: null;
	const isAccessReady =
		accessInitForUserId === userId &&
		!isAccessDataLoading &&
		!nodesQuery.isError &&
		!endpointsQuery.isError &&
		!grantsQuery.isError;

	async function applyAccessMatrix() {
		if (!adminToken || !userId || !isAccessReady) return;
		setIsApplyingAccess(true);
		setAccessError(null);
		try {
			const items: PutAdminUserGrantItem[] = selectedEndpointIds.map(
				(endpointId) => {
					const existing = grantByEndpoint.get(endpointId);
					return {
						endpoint_id: endpointId,
						enabled: true,
						quota_limit_bytes: existing?.quota_limit_bytes ?? 0,
						note: existing?.note ?? null,
					};
				},
			);
			await putAdminUserGrants(adminToken, userId, { items });
			await grantsQuery.refetch();
			pushToast({
				variant: "success",
				message: `Access updated (${items.length} endpoint${items.length === 1 ? "" : "s"})`,
			});
		} catch (error) {
			setAccessError(formatError(error));
		} finally {
			setIsApplyingAccess(false);
		}
	}

	async function loadSubscriptionPreview() {
		if (!subscriptionToken) return;
		setSubLoading(true);
		setSubError(null);
		try {
			const text = await fetchSubscription(subscriptionToken, subFormat);
			setSubText(text);
		} catch (error) {
			setSubError(formatError(error));
			setSubText("");
		} finally {
			setSubLoading(false);
		}
	}

	async function retryAccessData() {
		await Promise.all([
			nodesQuery.refetch(),
			endpointsQuery.refetch(),
			grantsQuery.refetch(),
		]);
	}

	async function saveUserProfile() {
		if (!adminToken || !userId) return;
		const normalizedDisplayName = displayName.trim();
		if (normalizedDisplayName.length === 0) {
			setUserSaveError("Display name is required.");
			return;
		}
		if (
			resetPolicy === "monthly" &&
			(!Number.isInteger(resetDay) || resetDay < 1 || resetDay > 31)
		) {
			setUserSaveError("Day of month must be between 1 and 31.");
			return;
		}
		if (
			!Number.isInteger(resetTzOffsetMinutes) ||
			resetTzOffsetMinutes < -720 ||
			resetTzOffsetMinutes > 840
		) {
			setUserSaveError("TZ offset must be between -720 and 840 minutes.");
			return;
		}

		setIsSavingUser(true);
		setUserSaveError(null);
		try {
			const quotaReset: UserQuotaReset =
				resetPolicy === "monthly"
					? {
							policy: "monthly",
							day_of_month: resetDay,
							tz_offset_minutes: resetTzOffsetMinutes,
						}
					: {
							policy: "unlimited",
							tz_offset_minutes: resetTzOffsetMinutes,
						};
			await patchAdminUser(adminToken, userId, {
				display_name: normalizedDisplayName,
				quota_reset: quotaReset,
			});
			await userQuery.refetch();
			pushToast({ variant: "success", message: "User updated" });
		} catch (error) {
			setUserSaveError(formatError(error));
		} finally {
			setIsSavingUser(false);
		}
	}

	async function confirmResetToken() {
		if (!adminToken || !userId) return;
		setIsResettingToken(true);
		try {
			const result = await resetAdminUserToken(adminToken, userId);
			await userQuery.refetch();
			pushToast({
				variant: "success",
				message: `Subscription token reset: ${result.subscription_token}`,
			});
			setResetTokenOpen(false);
		} catch (error) {
			pushToast({
				variant: "error",
				message: `Failed to reset token: ${formatError(error)}`,
			});
		} finally {
			setIsResettingToken(false);
		}
	}

	async function confirmDeleteUser() {
		if (!adminToken || !userId) return;
		setIsDeleting(true);
		try {
			await deleteAdminUser(adminToken, userId);
			pushToast({ variant: "success", message: "User deleted" });
			navigate({ to: "/users" });
		} catch (error) {
			pushToast({
				variant: "error",
				message: `Failed to delete user: ${formatError(error)}`,
			});
		} finally {
			setIsDeleting(false);
			setDeleteOpen(false);
		}
	}

	if (userQuery.isLoading) {
		return <PageState variant="loading" title="Loading user" />;
	}
	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to manage user details."
			/>
		);
	}
	if (userQuery.isError) {
		return (
			<PageState
				variant="error"
				title="Failed to load user"
				description={formatError(userQuery.error)}
			/>
		);
	}
	if (!user) {
		return <PageState variant="empty" title="User not found" />;
	}

	const nodeCards = nodesQuery.data?.items ?? [];
	const nodeQuotasByNodeId = new Map(
		(nodeQuotasQuery.data?.items ?? []).map((quota) => [quota.node_id, quota]),
	);

	return (
		<div className="space-y-6">
			<PageHeader
				title={user.display_name}
				description="Manage profile, grants, and quota status"
				actions={
					<div className="flex items-center gap-2">
						<Button variant="ghost" onClick={() => setResetTokenOpen(true)}>
							Reset token
						</Button>
						<Button variant="danger" onClick={() => setDeleteOpen(true)}>
							Delete user
						</Button>
					</div>
				}
			/>

			<div className="tabs tabs-boxed">
				<button
					type="button"
					className={`tab ${tab === "user" ? "tab-active" : ""}`}
					onClick={() => setTab("user")}
				>
					User
				</button>
				<button
					type="button"
					className={`tab ${tab === "access" ? "tab-active" : ""}`}
					onClick={() => setTab("access")}
				>
					Access
				</button>
				<button
					type="button"
					className={`tab ${tab === "quotaStatus" ? "tab-active" : ""}`}
					onClick={() => setTab("quotaStatus")}
				>
					Quota status
				</button>
			</div>

			{tab === "user" ? (
				<div className="space-y-6">
					<div className="rounded-box border border-base-300 bg-base-100 p-4 space-y-3">
						<label className="form-control gap-2">
							<span className="label-text">Display name</span>
							<input
								className="input input-bordered"
								value={displayName}
								onChange={(event) => setDisplayName(event.target.value)}
							/>
						</label>

						<div className="grid gap-3 md:grid-cols-3">
							<label className="form-control gap-2">
								<span className="label-text">Quota reset policy</span>
								<select
									className="select select-bordered"
									value={resetPolicy}
									onChange={(event) =>
										setResetPolicy(
											event.target.value as "monthly" | "unlimited",
										)
									}
								>
									<option value="monthly">monthly</option>
									<option value="unlimited">unlimited</option>
								</select>
							</label>

							<label className="form-control gap-2">
								<span className="label-text">Day of month</span>
								<input
									type="number"
									className="input input-bordered"
									min={1}
									max={31}
									disabled={resetPolicy !== "monthly"}
									value={resetDay}
									onChange={(event) =>
										setResetDay(Number(event.target.value || "1"))
									}
								/>
							</label>

							<label className="form-control gap-2">
								<span className="label-text">TZ offset (minutes)</span>
								<input
									type="number"
									className="input input-bordered"
									value={resetTzOffsetMinutes}
									onChange={(event) =>
										setResetTzOffsetMinutes(Number(event.target.value || "0"))
									}
								/>
							</label>
						</div>

						<div className="flex items-center gap-3 text-sm">
							<span className="font-medium">User ID:</span>
							<span className="font-mono">{user.user_id}</span>
						</div>
						<div className="flex items-center gap-3 text-sm">
							<span className="font-medium">Subscription token:</span>
							<span className="font-mono break-all">
								{user.subscription_token}
							</span>
						</div>
						<div className="rounded-box border border-base-200 p-3 space-y-3">
							<div className="flex flex-wrap items-end gap-3">
								<label className="form-control gap-2">
									<span className="label-text">Subscription format</span>
									<select
										className="select select-bordered"
										data-testid="subscription-format"
										value={subFormat}
										onChange={(event) =>
											setSubFormat(event.target.value as SubscriptionFormat)
										}
									>
										<option value="raw">raw</option>
										<option value="clash">clash</option>
									</select>
								</label>
								<CopyButton
									text={subscriptionUrl}
									label="Copy URL"
									ariaLabel="Copy subscription URL"
									className="self-end"
								/>
								<Button
									className="self-end"
									data-testid="subscription-fetch"
									loading={subLoading}
									onClick={async () => {
										setSubOpen(true);
										await loadSubscriptionPreview();
									}}
								>
									Fetch
								</Button>
							</div>
							<div className="text-xs opacity-70">
								Preview opens in a modal and keeps subscription formatting
								unchanged.
							</div>
						</div>

						{userSaveError ? (
							<div className="alert alert-error py-2 text-sm">
								{userSaveError}
							</div>
						) : null}
						<div>
							<Button onClick={saveUserProfile} loading={isSavingUser}>
								Save user
							</Button>
						</div>
					</div>

					<div className="rounded-box border border-base-300 bg-base-100 p-4 space-y-3">
						<h3 className="font-semibold">Node quotas</h3>
						<div className="alert py-2 text-sm">
							Node quota editing is currently unavailable in this view.
						</div>
						{nodesQuery.isLoading ? (
							<PageState variant="loading" title="Loading nodes" />
						) : null}
						{nodesQuery.isError ? (
							<PageState
								variant="error"
								title="Failed to load nodes"
								description={formatError(nodesQuery.error)}
							/>
						) : null}
						{nodeQuotasQuery.isLoading ? (
							<PageState variant="loading" title="Loading node quotas" />
						) : null}
						{nodeQuotasQuery.isError ? (
							<PageState
								variant="error"
								title="Failed to load node quotas"
								description={formatError(nodeQuotasQuery.error)}
							/>
						) : null}
						{!nodeQuotasQuery.isLoading && !nodeQuotasQuery.isError
							? nodeCards.map((node) => {
									const quota = nodeQuotasByNodeId.get(node.node_id);
									return (
										<div
											key={node.node_id}
											className="flex items-center justify-between rounded-box border border-base-200 p-3"
										>
											<div>
												<div className="font-medium">{node.node_name}</div>
												<div className="text-xs opacity-70 font-mono">
													{node.node_id}
												</div>
											</div>
											<NodeQuotaEditor
												value={quota?.quota_limit_bytes ?? 0}
												onApply={() => Promise.resolve()}
												disabled
											/>
										</div>
									);
								})
							: null}
					</div>
				</div>
			) : null}

			{tab === "access" ? (
				<div className="space-y-4">
					<div className="flex items-center justify-between">
						<div className="text-sm opacity-70">
							Selected endpoints: {selectedEndpointIds.length}
						</div>
						<Button
							onClick={applyAccessMatrix}
							loading={isApplyingAccess}
							disabled={!isAccessReady}
						>
							Apply access
						</Button>
					</div>

					{accessError ? (
						<div className="alert alert-error py-2 text-sm">{accessError}</div>
					) : null}
					{isAccessDataLoading ? (
						<PageState variant="loading" title="Loading access matrix" />
					) : null}
					{accessDataError ? (
						<div className="space-y-3">
							<PageState
								variant="error"
								title="Failed to load access matrix"
								description={accessDataError}
							/>
							<Button variant="ghost" onClick={() => void retryAccessData()}>
								Retry access data
							</Button>
						</div>
					) : null}

					{!isAccessDataLoading && !accessDataError ? (
						<GrantAccessMatrix
							nodes={(nodesQuery.data?.items ?? []).map((node) => ({
								nodeId: node.node_id,
								label: node.node_name,
								details: (
									<span className="text-xs opacity-60 font-mono">
										{node.node_id}
									</span>
								),
							}))}
							protocols={PROTOCOLS.map((protocol) => ({
								protocolId: protocol.protocolId,
								label: protocol.label,
							}))}
							cells={cells}
							disabled={isAccessDataLoading || !isAccessReady}
							onToggleCell={(nodeId, protocolId) =>
								toggleCell(nodeId, protocolId as SupportedProtocolId)
							}
							onToggleRow={toggleRow}
							onToggleColumn={(protocolId) =>
								toggleColumn(protocolId as SupportedProtocolId)
							}
							onToggleAll={toggleAll}
							onSelectCellEndpoint={(nodeId, protocolId, endpointId) =>
								setCellEndpoint(
									nodeId,
									protocolId as SupportedProtocolId,
									endpointId,
								)
							}
						/>
					) : null}
				</div>
			) : null}

			{tab === "quotaStatus" ? (
				<div className="rounded-box border border-base-300 bg-base-100 p-4 space-y-3">
					{nodeQuotaStatusQuery.isLoading ? (
						<PageState variant="loading" title="Loading quota status" />
					) : null}
					{nodeQuotaStatusQuery.isError ? (
						<PageState
							variant="error"
							title="Failed to load quota status"
							description={formatError(nodeQuotaStatusQuery.error)}
						/>
					) : null}
					{(nodeQuotaStatusQuery.data?.items ?? []).map((item) => {
						const isUnlimited = item.quota_limit_bytes === 0;
						const quotaLimitText = isUnlimited
							? "unlimited"
							: formatQuotaBytesHuman(item.quota_limit_bytes);
						const remainingText = isUnlimited
							? "unlimited"
							: formatQuotaBytesHuman(item.remaining_bytes);
						return (
							<div
								key={`${item.node_id}::${item.user_id}`}
								className="rounded-box border border-base-200 p-3 space-y-1"
							>
								<div className="font-medium">{item.node_id}</div>
								<div className="text-sm">
									Used {formatQuotaBytesHuman(item.used_bytes)} /{" "}
									{quotaLimitText}
								</div>
								<div className="text-sm opacity-70">
									Remaining: {remainingText}
								</div>
							</div>
						);
					})}
				</div>
			) : null}

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
				title="Reset subscription token"
				description="This invalidates the old token immediately."
				confirmLabel={isResettingToken ? "Resetting..." : "Reset"}
				cancelLabel="Cancel"
				onCancel={() => setResetTokenOpen(false)}
				onConfirm={confirmResetToken}
			/>

			<ConfirmDialog
				open={deleteOpen}
				title="Delete user"
				description="This cannot be undone."
				cancelLabel="Cancel"
				onCancel={() => setDeleteOpen(false)}
				footer={
					<div className="modal-action">
						<button
							type="button"
							className="btn"
							disabled={isDeleting}
							onClick={() => setDeleteOpen(false)}
						>
							Cancel
						</button>
						<button
							type="button"
							className="btn btn-error"
							disabled={isDeleting}
							onClick={confirmDeleteUser}
						>
							{isDeleting ? "Deleting..." : "Delete"}
						</button>
					</div>
				}
			/>

			<div className="text-xs opacity-60">
				Tip: use the Access tab to control endpoint membership directly in
				user/node/endpoint mode.
			</div>
			<Link to="/users" className="btn btn-ghost btn-sm">
				Back to users
			</Link>
		</div>
	);
}
