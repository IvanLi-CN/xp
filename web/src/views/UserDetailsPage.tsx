import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState } from "react";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import {
	createAdminGrant,
	deleteAdminGrant,
	fetchAdminGrants,
} from "../api/adminGrants";
import { fetchAdminNodes } from "../api/adminNodes";
import {
	fetchAdminUserNodeQuotas,
	putAdminUserNodeQuota,
} from "../api/adminUserNodeQuotas";
import {
	type AdminUserPatchRequest,
	type CyclePolicyDefault,
	deleteAdminUser,
	fetchAdminUser,
	patchAdminUser,
	resetAdminUserToken,
} from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";
import { fetchSubscription } from "../api/subscription";
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
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

type SubscriptionFormat = "base64" | "raw" | "clash";

function isValidRawSubscription(content: string): boolean {
	const trimmed = content.trim();
	return (
		trimmed.startsWith("vless://") ||
		trimmed.startsWith("ss://") ||
		trimmed.includes("://")
	);
}

export function UserDetailsPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
	const { userId } = useParams({ from: "/app/users/$userId" });
	const { pushToast } = useToast();
	const queryClient = useQueryClient();
	const prefs = useUiPrefs();

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";
	const selectClass =
		prefs.density === "compact"
			? "select select-bordered select-sm"
			: "select select-bordered";
	const textareaClass =
		prefs.density === "compact"
			? "textarea textarea-bordered textarea-sm h-40 font-mono text-xs"
			: "textarea textarea-bordered h-40 font-mono text-xs";

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
		queryKey: ["adminGrants", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminGrants(adminToken, signal),
	});

	const nodeQuotasQuery = useQuery({
		queryKey: ["adminUserNodeQuotas", adminToken, userId],
		enabled: adminToken.length > 0 && userId.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminUserNodeQuotas(adminToken, userId, signal),
	});

	const [displayName, setDisplayName] = useState("");
	const [cyclePolicy, setCyclePolicy] = useState<CyclePolicyDefault>("by_user");
	const [cycleDay, setCycleDay] = useState(1);
	const [formError, setFormError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);
	const [resetTokenOpen, setResetTokenOpen] = useState(false);
	const [isResettingToken, setIsResettingToken] = useState(false);
	const [deleteOpen, setDeleteOpen] = useState(false);
	const [isDeleting, setIsDeleting] = useState(false);

	const [subscriptionFormat, setSubscriptionFormat] =
		useState<SubscriptionFormat>("base64");
	const [subscriptionContent, setSubscriptionContent] = useState("");
	const [subscriptionError, setSubscriptionError] = useState<string | null>(
		null,
	);
	const [isFetchingSubscription, setIsFetchingSubscription] = useState(false);

	const [nodeFilter, setNodeFilter] = useState("");
	const [selectedByCell, setSelectedByCell] = useState<Record<string, string>>(
		{},
	);
	const [matrixInitialized, setMatrixInitialized] = useState(false);
	const [matrixError, setMatrixError] = useState<string | null>(null);
	const [isSavingMatrix, setIsSavingMatrix] = useState(false);

	useEffect(() => {
		if (!userQuery.data) return;
		setDisplayName(userQuery.data.display_name);
		setCyclePolicy(userQuery.data.cycle_policy_default);
		setCycleDay(userQuery.data.cycle_day_of_month_default);
	}, [userQuery.data]);

	const PROTOCOLS = useMemo(
		() =>
			[
				{ protocolId: "vless_reality_vision_tcp", label: "VLESS" },
				{ protocolId: "ss2022_2022_blake3_aes_128_gcm", label: "SS2022" },
			] as const,
		[],
	);

	const cellKey = useCallback(
		(nodeId: string, protocolId: string) => `${nodeId}::${protocolId}`,
		[],
	);

	const accessDeps = useMemo(() => {
		const nodes = nodesQuery.data?.items ?? [];
		const endpoints = endpointsQuery.data?.items ?? [];
		const grants = grantsQuery.data?.items ?? [];
		const nodeQuotas = nodeQuotasQuery.data?.items ?? [];
		return { nodes, endpoints, grants, nodeQuotas };
	}, [
		endpointsQuery.data?.items,
		grantsQuery.data?.items,
		nodeQuotasQuery.data?.items,
		nodesQuery.data?.items,
	]);

	const endpointsById = useMemo(() => {
		return new Map(accessDeps.endpoints.map((ep) => [ep.endpoint_id, ep]));
	}, [accessDeps.endpoints]);

	const endpointsByNodeProtocol = useMemo(() => {
		const map = new Map<
			string,
			Map<string, Array<(typeof accessDeps.endpoints)[number]>>
		>();
		for (const ep of accessDeps.endpoints) {
			const supported = PROTOCOLS.some((p) => p.protocolId === ep.kind);
			if (!supported) continue;
			if (!map.has(ep.node_id)) map.set(ep.node_id, new Map());
			const byProtocol = map.get(ep.node_id);
			if (!byProtocol) continue;
			if (!byProtocol.has(ep.kind)) byProtocol.set(ep.kind, []);
			byProtocol.get(ep.kind)?.push(ep);
		}
		for (const [, byProtocol] of map) {
			for (const [, list] of byProtocol) {
				list.sort((a, b) => a.port - b.port || a.tag.localeCompare(b.tag));
			}
		}
		return map;
	}, [PROTOCOLS, accessDeps.endpoints]);

	const userGrants = useMemo(() => {
		return accessDeps.grants.filter((g) => g.user_id === userId);
	}, [accessDeps.grants, userId]);

	const initialSelectedByCell = useMemo(() => {
		const candidates = new Map<string, string[]>();
		for (const g of userGrants) {
			const ep = endpointsById.get(g.endpoint_id);
			if (!ep) continue;
			const supported = PROTOCOLS.some((p) => p.protocolId === ep.kind);
			if (!supported) continue;
			const key = cellKey(ep.node_id, ep.kind);
			const list = candidates.get(key) ?? [];
			list.push(ep.endpoint_id);
			candidates.set(key, list);
		}

		const out: Record<string, string> = {};
		for (const [key, endpointIds] of candidates) {
			const sorted = endpointIds
				.map((id) => endpointsById.get(id))
				.filter(Boolean)
				.sort((a, b) => {
					if (!a || !b) return 0;
					return a.port - b.port || a.tag.localeCompare(b.tag);
				})
				.map((ep) => ep?.endpoint_id)
				.filter(Boolean) as string[];

			const first = sorted[0] ?? endpointIds[0];
			if (first) out[key] = first;
		}
		return out;
	}, [PROTOCOLS, cellKey, endpointsById, userGrants]);

	useEffect(() => {
		if (matrixInitialized) return;
		const accessLoaded = Boolean(
			nodesQuery.data &&
				endpointsQuery.data &&
				grantsQuery.data &&
				nodeQuotasQuery.data,
		);
		if (!accessLoaded) return;
		setSelectedByCell(initialSelectedByCell);
		setMatrixInitialized(true);
	}, [
		endpointsQuery.data,
		grantsQuery.data,
		initialSelectedByCell,
		matrixInitialized,
		nodeQuotasQuery.data,
		nodesQuery.data,
	]);

	const nodeQuotaByNodeId = useMemo(() => {
		const map = new Map<string, number>();
		for (const q of accessDeps.nodeQuotas) {
			map.set(q.node_id, q.quota_limit_bytes);
		}
		return map;
	}, [accessDeps.nodeQuotas]);

	const userGrantsByNodeId = useMemo(() => {
		const map = new Map<string, typeof userGrants>();
		for (const g of userGrants) {
			const ep = endpointsById.get(g.endpoint_id);
			if (!ep) continue;
			if (!map.has(ep.node_id)) map.set(ep.node_id, []);
			map.get(ep.node_id)?.push(g);
		}
		return map;
	}, [endpointsById, userGrants]);

	const nodeQuotaValueForNode = (nodeId: string): number | "mixed" => {
		const explicit = nodeQuotaByNodeId.get(nodeId);
		if (explicit !== undefined) return explicit;
		const grants = userGrantsByNodeId.get(nodeId) ?? [];
		const values = grants
			.map((g) => g.quota_limit_bytes)
			.filter((v) => typeof v === "number");
		if (values.length === 0) return 0;
		const first = values[0];
		return values.every((v) => v === first) ? first : "mixed";
	};

	const visibleNodes = useMemo(() => {
		const q = nodeFilter.trim().toLowerCase();
		if (!q) return accessDeps.nodes;
		return accessDeps.nodes.filter(
			(n) =>
				n.node_name.toLowerCase().includes(q) ||
				n.node_id.toLowerCase().includes(q),
		);
	}, [accessDeps.nodes, nodeFilter]);

	const grantByEndpointId = useMemo(() => {
		return new Map(userGrants.map((g) => [g.endpoint_id, g]));
	}, [userGrants]);

	const cells = useMemo(() => {
		const out: Record<string, Record<string, GrantAccessMatrixCellState>> = {};
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
				const selectedEpId = selectedByCell[key] ?? null;
				const selectedEp = selectedEpId
					? (options.find((ep) => ep.endpoint_id === selectedEpId) ?? null)
					: null;
				const selectedGrant = selectedEp
					? (grantByEndpointId.get(selectedEp.endpoint_id) ?? null)
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
									grantId: selectedGrant?.grant_id,
								}
							: {
									endpointId: options[0].endpoint_id,
									tag: options[0].tag,
									port: options[0].port,
									grantId: selectedGrant?.grant_id,
								},
				};
			}
			out[n.node_id] = row;
		}
		return out;
	}, [
		PROTOCOLS,
		cellKey,
		endpointsByNodeProtocol,
		grantByEndpointId,
		selectedByCell,
		visibleNodes,
	]);

	const subscriptionToken = userQuery.data?.subscription_token ?? "";
	const subscriptionUrl = useMemo(() => {
		if (!subscriptionToken) return "";
		const baseUrl = typeof window === "undefined" ? "" : window.location.origin;
		const params = new URLSearchParams();
		if (subscriptionFormat !== "base64") {
			params.set("format", subscriptionFormat);
		}
		const query = params.toString();
		const path = `/api/sub/${encodeURIComponent(subscriptionToken)}`;
		if (baseUrl) {
			return query ? `${baseUrl}${path}?${query}` : `${baseUrl}${path}`;
		}
		return query ? `${path}?${query}` : path;
	}, [subscriptionFormat, subscriptionToken]);

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
				description="Fetching user details from the control plane."
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

	if (!userQuery.data) {
		return (
			<PageState
				variant="empty"
				title="User not found"
				description="The user ID does not exist."
				action={
					<Link to="/users" className="btn btn-outline btn-sm xp-btn-outline">
						Back to users
					</Link>
				}
			/>
		);
	}

	const user = userQuery.data;
	const hasChanges =
		displayName.trim() !== user.display_name ||
		cyclePolicy !== user.cycle_policy_default ||
		cycleDay !== user.cycle_day_of_month_default;

	return (
		<div className="space-y-6">
			<PageHeader
				title="User details"
				description={
					<>
						User ID: <span className="font-mono">{user.user_id}</span>
					</>
				}
				actions={
					<Link to="/users" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>

			<form
				className="card bg-base-100 shadow"
				onSubmit={async (event) => {
					event.preventDefault();
					if (!hasChanges) {
						pushToast({
							variant: "info",
							message: "No changes to save.",
						});
						return;
					}
					if (!displayName.trim()) {
						setFormError("Display name is required.");
						return;
					}
					if (cycleDay < 1 || cycleDay > 31) {
						setFormError("Cycle day must be between 1 and 31.");
						return;
					}
					setFormError(null);
					setIsSaving(true);
					try {
						const payload: AdminUserPatchRequest = {};
						if (displayName.trim() !== user.display_name) {
							payload.display_name = displayName.trim();
						}
						if (cyclePolicy !== user.cycle_policy_default) {
							payload.cycle_policy_default = cyclePolicy;
						}
						if (cycleDay !== user.cycle_day_of_month_default) {
							payload.cycle_day_of_month_default = cycleDay;
						}
						const updated = await patchAdminUser(
							adminToken,
							user.user_id,
							payload,
						);
						queryClient.setQueryData(
							["adminUser", adminToken, userId],
							updated,
						);
						pushToast({
							variant: "success",
							message: "User updated.",
						});
					} catch (err) {
						setFormError(formatError(err));
						pushToast({
							variant: "error",
							message: "Failed to update user.",
						});
					} finally {
						setIsSaving(false);
					}
				}}
			>
				<div className="card-body space-y-4">
					<h2 className="card-title">Profile</h2>
					<label className="form-control">
						<div className="label">
							<span className="label-text">Display name</span>
						</div>
						<input
							className={inputClass}
							value={displayName}
							onChange={(event) => setDisplayName(event.target.value)}
						/>
					</label>
					<div className="grid gap-4 md:grid-cols-2">
						<label className="form-control">
							<div className="label">
								<span className="label-text">Cycle policy</span>
							</div>
							<select
								className={selectClass}
								value={cyclePolicy}
								onChange={(event) =>
									setCyclePolicy(event.target.value as CyclePolicyDefault)
								}
							>
								<option value="by_user">by_user</option>
								<option value="by_node">by_node</option>
							</select>
						</label>
						<label className="form-control">
							<div className="label">
								<span className="label-text">Cycle day of month</span>
							</div>
							<input
								className={inputClass}
								type="number"
								min={1}
								max={31}
								value={cycleDay}
								onChange={(event) => setCycleDay(Number(event.target.value))}
							/>
						</label>
					</div>
					{formError ? <p className="text-sm text-error">{formError}</p> : null}
					<div className="card-actions justify-end">
						<Button type="submit" loading={isSaving}>
							Save changes
						</Button>
					</div>
				</div>
			</form>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<h2 className="card-title">Access & quota</h2>

					{nodesQuery.isLoading ||
					endpointsQuery.isLoading ||
					grantsQuery.isLoading ||
					nodeQuotasQuery.isLoading ? (
						<p className="text-sm opacity-70">
							Loading nodes, endpoints, grants and node quotas…
						</p>
					) : nodesQuery.isError ||
						endpointsQuery.isError ||
						grantsQuery.isError ||
						nodeQuotasQuery.isError ? (
						<div className="space-y-2">
							<p className="text-sm text-error">
								Failed to load access data:{" "}
								{nodesQuery.isError
									? formatError(nodesQuery.error)
									: endpointsQuery.isError
										? formatError(endpointsQuery.error)
										: grantsQuery.isError
											? formatError(grantsQuery.error)
											: nodeQuotasQuery.isError
												? formatError(nodeQuotasQuery.error)
												: "Unknown error"}
							</p>
							<Button
								variant="secondary"
								onClick={() => {
									nodesQuery.refetch();
									endpointsQuery.refetch();
									grantsQuery.refetch();
									nodeQuotasQuery.refetch();
								}}
							>
								Retry
							</Button>
						</div>
					) : accessDeps.nodes.length === 0 ? (
						<p className="text-sm opacity-70">
							Create at least one node and endpoint to configure access.
						</p>
					) : (
						<>
							<div className="flex flex-col gap-3 md:flex-row md:items-center">
								<input
									className={[
										inputClass,
										"w-full md:max-w-sm bg-base-200/30",
									].join(" ")}
									placeholder="Filter nodes..."
									value={nodeFilter}
									onChange={(event) => setNodeFilter(event.target.value)}
								/>
								<div className="flex items-center gap-2">
									{(() => {
										const totalCells = visibleNodes.reduce((sum, n) => {
											let count = 0;
											for (const p of PROTOCOLS) {
												const options =
													endpointsByNodeProtocol
														.get(n.node_id)
														?.get(p.protocolId) ?? [];
												if (options.length > 0) count += 1;
											}
											return sum + count;
										}, 0);
										const selectedCells = visibleNodes.reduce((sum, n) => {
											let count = 0;
											for (const p of PROTOCOLS) {
												const options =
													endpointsByNodeProtocol
														.get(n.node_id)
														?.get(p.protocolId) ?? [];
												if (options.length === 0) continue;
												if (selectedByCell[cellKey(n.node_id, p.protocolId)])
													count += 1;
											}
											return sum + count;
										}, 0);
										return (
											<span className="rounded-full border border-base-200 bg-base-200/40 px-4 py-2 font-mono text-xs">
												Selected {selectedCells} / {totalCells}
											</span>
										);
									})()}
								</div>
								<div className="flex-1" />
								<Button
									variant="secondary"
									size="sm"
									onClick={() => {
										setSelectedByCell(initialSelectedByCell);
										setMatrixError(null);
									}}
									disabled={isSavingMatrix}
								>
									Reset
								</Button>
								<Button
									size="sm"
									loading={isSavingMatrix}
									disabled={isSavingMatrix}
									onClick={async () => {
										if (isSavingMatrix) return;
										setIsSavingMatrix(true);
										setMatrixError(null);
										try {
											const desiredByCell = new Map<string, string>();
											for (const [k, v] of Object.entries(selectedByCell)) {
												desiredByCell.set(k, v);
											}

											const existingByCell = new Map<
												string,
												Array<{ grant_id: string; endpoint_id: string }>
											>();
											for (const g of userGrants) {
												const ep = endpointsById.get(g.endpoint_id);
												if (!ep) continue;
												const supported = PROTOCOLS.some(
													(p) => p.protocolId === ep.kind,
												);
												if (!supported) continue;
												const k = cellKey(ep.node_id, ep.kind);
												if (!existingByCell.has(k)) existingByCell.set(k, []);
												existingByCell.get(k)?.push({
													grant_id: g.grant_id,
													endpoint_id: g.endpoint_id,
												});
											}

											const toDelete: string[] = [];
											const toCreate: string[] = [];

											// Deletions and "unify to one endpoint per cell".
											for (const [k, grants] of existingByCell) {
												const desiredEndpointId = desiredByCell.get(k);
												if (!desiredEndpointId) {
													for (const g of grants) toDelete.push(g.grant_id);
													continue;
												}
												const keep = grants.find(
													(g) => g.endpoint_id === desiredEndpointId,
												);
												for (const g of grants) {
													if (!keep || g.endpoint_id !== desiredEndpointId) {
														toDelete.push(g.grant_id);
													}
												}
												if (!keep) {
													toCreate.push(desiredEndpointId);
												}
											}

											// Creations for cells that had no existing grants.
											for (const [k, desiredEndpointId] of desiredByCell) {
												if (existingByCell.has(k)) continue;
												toCreate.push(desiredEndpointId);
											}

											for (const endpointId of toCreate) {
												const ep = endpointsById.get(endpointId);
												if (!ep) continue;
												const quota =
													nodeQuotaByNodeId.get(ep.node_id) ??
													nodeQuotaValueForNode(ep.node_id) ??
													0;
												await createAdminGrant(adminToken, {
													user_id: user.user_id,
													endpoint_id: endpointId,
													quota_limit_bytes:
														typeof quota === "number" ? quota : 0,
													cycle_policy: "inherit_user",
													cycle_day_of_month: null,
													note: null,
												});
											}

											for (const grantId of toDelete) {
												await deleteAdminGrant(adminToken, grantId);
											}

											await Promise.all([
												grantsQuery.refetch(),
												nodeQuotasQuery.refetch(),
											]);
											setMatrixInitialized(false);
											pushToast({
												variant: "success",
												message: "Access matrix updated.",
											});
										} catch (err) {
											const message = formatError(err);
											setMatrixError(message);
											pushToast({
												variant: "error",
												message: "Failed to update access matrix.",
											});
										} finally {
											setIsSavingMatrix(false);
										}
									}}
								>
									Save changes
								</Button>
							</div>
							<div className="text-xs opacity-60 space-y-1">
								<div>
									Node quota input: MiB/GiB (default MiB). Accepts M/G. GB/MB
									treated as GiB/MiB.
								</div>
								<div>Quota applies to the node across protocols.</div>
							</div>

							<GrantAccessMatrix
								nodes={visibleNodes.map((n) => ({
									nodeId: n.node_id,
									label: n.node_name,
									details: (
										<NodeQuotaEditor
											value={nodeQuotaValueForNode(n.node_id)}
											onApply={async (nextBytes) => {
												try {
													await putAdminUserNodeQuota(
														adminToken,
														user.user_id,
														n.node_id,
														nextBytes,
													);
													await Promise.all([
														nodeQuotasQuery.refetch(),
														grantsQuery.refetch(),
													]);
													pushToast({
														variant: "success",
														message: "Node quota updated.",
													});
												} catch (err) {
													throw new Error(formatError(err));
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
								onToggleCell={(nodeId, protocolId) => {
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
								}}
								onToggleRow={(nodeId) => {
									const protocolIds = PROTOCOLS.map((p) => p.protocolId);
									setSelectedByCell((prev) => {
										const hasAny = protocolIds.some((pid) =>
											Boolean(prev[cellKey(nodeId, pid)]),
										);
										const next = { ...prev };
										for (const pid of protocolIds) {
											const key = cellKey(nodeId, pid);
											const options =
												endpointsByNodeProtocol.get(nodeId)?.get(pid) ?? [];
											if (options.length === 0) continue;
											if (hasAny) delete next[key];
											else next[key] = options[0].endpoint_id;
										}
										return next;
									});
								}}
								onToggleColumn={(protocolId) => {
									setSelectedByCell((prev) => {
										const hasAny = visibleNodes.some((n) =>
											Boolean(prev[cellKey(n.node_id, protocolId)]),
										);
										const next = { ...prev };
										for (const n of visibleNodes) {
											const key = cellKey(n.node_id, protocolId);
											const options =
												endpointsByNodeProtocol
													.get(n.node_id)
													?.get(protocolId) ?? [];
											if (options.length === 0) continue;
											if (hasAny) delete next[key];
											else next[key] = options[0].endpoint_id;
										}
										return next;
									});
								}}
								onToggleAll={() => {
									setSelectedByCell((prev) => {
										const hasAny = Object.keys(prev).length > 0;
										if (hasAny) return {};
										const next: Record<string, string> = {};
										for (const n of visibleNodes) {
											for (const p of PROTOCOLS) {
												const key = cellKey(n.node_id, p.protocolId);
												const options =
													endpointsByNodeProtocol
														.get(n.node_id)
														?.get(p.protocolId) ?? [];
												if (options.length === 0) continue;
												next[key] = options[0].endpoint_id;
											}
										}
										return next;
									});
								}}
								onSelectCellEndpoint={(nodeId, protocolId, endpointId) => {
									const options =
										endpointsByNodeProtocol.get(nodeId)?.get(protocolId) ?? [];
									if (!options.some((ep) => ep.endpoint_id === endpointId))
										return;
									const key = cellKey(nodeId, protocolId);
									setSelectedByCell((prev) => ({ ...prev, [key]: endpointId }));
								}}
							/>

							{matrixError ? (
								<p className="text-sm text-error">{matrixError}</p>
							) : null}
						</>
					)}
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-3">
					<h2 className="card-title">Subscription</h2>
					<div className="space-y-1">
						<p className="text-sm opacity-70">Subscription token</p>
						<p className="font-mono text-xs break-all">{subscriptionToken}</p>
					</div>
					<div className="flex flex-wrap gap-3">
						<label className="form-control">
							<div className="label">
								<span className="label-text">Format</span>
							</div>
							<select
								className={selectClass}
								value={subscriptionFormat}
								onChange={(event) => {
									setSubscriptionFormat(
										event.target.value as SubscriptionFormat,
									);
									setSubscriptionContent("");
									setSubscriptionError(null);
								}}
							>
								<option value="base64">base64</option>
								<option value="raw">raw</option>
								<option value="clash">clash</option>
							</select>
						</label>
						<div className="flex flex-wrap items-end gap-2">
							<CopyButton
								text={subscriptionUrl}
								label="Copy URL"
								variant="secondary"
							/>
							<Button
								variant="secondary"
								loading={isFetchingSubscription}
								onClick={async () => {
									if (!subscriptionToken) return;
									setIsFetchingSubscription(true);
									setSubscriptionError(null);
									try {
										const formatParam =
											subscriptionFormat === "base64"
												? undefined
												: subscriptionFormat;
										const content = await fetchSubscription(
											subscriptionToken,
											formatParam,
										);
										const trimmed = content.trim();
										if (!trimmed) {
											throw new Error("Subscription content is empty.");
										}
										if (
											subscriptionFormat === "raw" &&
											!isValidRawSubscription(trimmed)
										) {
											throw new Error(
												"Raw subscription does not look like a URI list.",
											);
										}
										setSubscriptionContent(content);
										pushToast({
											variant: "success",
											message: "Subscription fetched.",
										});
									} catch (err) {
										const message = formatError(err);
										setSubscriptionError(message);
										pushToast({
											variant: "error",
											message: "Failed to fetch subscription.",
										});
									} finally {
										setIsFetchingSubscription(false);
									}
								}}
							>
								Fetch subscription
							</Button>
							{subscriptionContent ? (
								<CopyButton
									text={subscriptionContent}
									label="Copy content"
									variant="ghost"
								/>
							) : null}
						</div>
					</div>
					{subscriptionError ? (
						<p className="text-sm text-error">{subscriptionError}</p>
					) : null}
					{subscriptionContent ? (
						<textarea
							className={textareaClass}
							value={subscriptionContent}
							readOnly
						/>
					) : (
						<p className="text-sm opacity-70">
							Fetch the subscription content to preview and copy it.
						</p>
					)}
					<div className="card-actions justify-end">
						<Button
							variant="ghost"
							disabled={isResettingToken || !subscriptionToken}
							onClick={() => setResetTokenOpen(true)}
						>
							Reset token
						</Button>
					</div>
				</div>
			</div>

			<div className="card bg-base-100 shadow border border-error/30">
				<div className="card-body space-y-3">
					<h2 className="card-title text-error">Danger zone</h2>
					<p className="text-sm opacity-70">
						Deleting a user removes all associated grants.
					</p>
					<div className="card-actions justify-end">
						<Button variant="ghost" onClick={() => setDeleteOpen(true)}>
							Delete user
						</Button>
					</div>
				</div>
			</div>

			<ConfirmDialog
				open={deleteOpen}
				title="Delete user"
				description="This action cannot be undone. Are you sure?"
				confirmLabel={isDeleting ? "Deleting..." : "Delete"}
				onCancel={() => setDeleteOpen(false)}
				onConfirm={async () => {
					setIsDeleting(true);
					try {
						await deleteAdminUser(adminToken, user.user_id);
						pushToast({
							variant: "success",
							message: "User deleted.",
						});
						navigate({ to: "/users" });
					} catch (err) {
						pushToast({
							variant: "error",
							message: `Failed to delete user: ${formatError(err)}`,
						});
					} finally {
						setIsDeleting(false);
						setDeleteOpen(false);
					}
				}}
			/>
			<ConfirmDialog
				open={resetTokenOpen}
				title="Reset subscription token"
				description="This will invalidate the previous token immediately. Existing subscription links will stop working. Continue?"
				confirmLabel={isResettingToken ? "Resetting..." : "Reset token"}
				onCancel={() => setResetTokenOpen(false)}
				onConfirm={async () => {
					if (isResettingToken) return;
					setIsResettingToken(true);
					setSubscriptionError(null);
					try {
						const refreshed = await resetAdminUserToken(
							adminToken,
							user.user_id,
						);
						queryClient.setQueryData(["adminUser", adminToken, userId], {
							...user,
							subscription_token: refreshed.subscription_token,
						});
						setSubscriptionContent("");
						pushToast({
							variant: "success",
							message: "Subscription token reset.",
						});
					} catch (err) {
						setSubscriptionError(formatError(err));
						pushToast({
							variant: "error",
							message: "Failed to reset token.",
						});
					} finally {
						setIsResettingToken(false);
						setResetTokenOpen(false);
					}
				}}
			/>
		</div>
	);
}
