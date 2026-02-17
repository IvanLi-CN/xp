import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import type { AdminNode } from "../api/adminNodes";
import { fetchAdminNodes } from "../api/adminNodes";
import {
	type AdminRealityDomain,
	createAdminRealityDomain,
	deleteAdminRealityDomain,
	fetchAdminRealityDomains,
	patchAdminRealityDomain,
	reorderAdminRealityDomains,
} from "../api/adminRealityDomains";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Icon } from "../components/Icon";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { TagInput } from "../components/TagInput";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";
import {
	normalizeRealityServerName,
	validateRealityServerName,
} from "../utils/realityServerName";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	if (error instanceof Error) return error.message;
	return String(error);
}

function toggleNodeId(list: string[], nodeId: string): string[] {
	return list.includes(nodeId)
		? list.filter((id) => id !== nodeId)
		: [...list, nodeId];
}

function moveItem<T>(items: T[], from: number, to: number): T[] {
	if (from === to) return items;
	if (from < 0 || from >= items.length) return items;
	if (to < 0 || to >= items.length) return items;
	const next = items.slice();
	const [removed] = next.splice(from, 1);
	next.splice(to, 0, removed);
	return next;
}

function nodeLabel(node: AdminNode): string {
	const name = node.node_name.trim();
	return name ? name : node.node_id;
}

type DomainDeleteTarget = {
	domainId: string;
	serverName: string;
};

export function RealityDomainsPage() {
	const queryClient = useQueryClient();
	const { pushToast } = useToast();
	const prefs = useUiPrefs();
	const adminToken = readAdminToken();

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const domainsQuery = useQuery({
		queryKey: ["adminRealityDomains", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminRealityDomains(adminToken, signal),
	});

	const nodes = nodesQuery.data?.items ?? [];
	const domains = domainsQuery.data?.items ?? [];

	const [createServerNames, setCreateServerNames] = useState<string[]>([]);
	const [deleteTarget, setDeleteTarget] = useState<DomainDeleteTarget | null>(
		null,
	);

	const createMutation = useMutation({
		mutationFn: async (serverNames: string[]) => {
			if (adminToken.length === 0) throw new Error("Missing admin token.");
			const normalized = serverNames
				.map(normalizeRealityServerName)
				.filter((name) => name.length > 0);
			if (normalized.length === 0) {
				throw new Error("serverName is required.");
			}
			for (const name of normalized) {
				const err = validateRealityServerName(name);
				if (err) throw new Error(err);
			}

			const created: AdminRealityDomain[] = [];
			for (const name of normalized) {
				created.push(
					await createAdminRealityDomain(adminToken, {
						server_name: name,
						disabled_node_ids: [],
					}),
				);
			}
			return created;
		},
		onSuccess: (created) => {
			setCreateServerNames([]);
			queryClient.setQueryData(
				["adminRealityDomains", adminToken],
				(prev: unknown) => {
					if (
						!prev ||
						typeof prev !== "object" ||
						!Array.isArray((prev as { items?: unknown }).items)
					) {
						return { items: created };
					}
					const items = (prev as { items: AdminRealityDomain[] }).items;
					return { items: [...items, ...created] };
				},
			);
			pushToast({
				variant: "success",
				message:
					created.length === 1
						? "Domain added."
						: `Added ${created.length} domains.`,
			});
		},
		onError: (error) => {
			pushToast({
				variant: "error",
				message: formatErrorMessage(error),
			});
		},
	});

	const patchMutation = useMutation({
		mutationFn: async (options: {
			domainId: string;
			disabledNodeIds: string[];
		}) => {
			if (adminToken.length === 0) throw new Error("Missing admin token.");
			return patchAdminRealityDomain(adminToken, options.domainId, {
				disabled_node_ids: options.disabledNodeIds,
			});
		},
		onSuccess: (updated) => {
			queryClient.setQueryData(
				["adminRealityDomains", adminToken],
				(prev: unknown) => {
					if (
						!prev ||
						typeof prev !== "object" ||
						!Array.isArray((prev as { items?: unknown }).items)
					) {
						return { items: [updated] };
					}
					const items = (prev as { items: AdminRealityDomain[] }).items;
					return {
						items: items.map((item) =>
							item.domain_id === updated.domain_id ? updated : item,
						),
					};
				},
			);
		},
		onError: (error) => {
			pushToast({
				variant: "error",
				message: formatErrorMessage(error),
			});
		},
	});

	const deleteMutation = useMutation({
		mutationFn: async (domainId: string) => {
			if (adminToken.length === 0) throw new Error("Missing admin token.");
			await deleteAdminRealityDomain(adminToken, domainId);
		},
		onSuccess: (_void, domainId) => {
			queryClient.setQueryData(
				["adminRealityDomains", adminToken],
				(prev: unknown) => {
					if (
						!prev ||
						typeof prev !== "object" ||
						!Array.isArray((prev as { items?: unknown }).items)
					) {
						return { items: [] };
					}
					const items = (prev as { items: AdminRealityDomain[] }).items;
					return { items: items.filter((item) => item.domain_id !== domainId) };
				},
			);
			pushToast({ variant: "success", message: "Domain deleted." });
		},
		onError: (error) => {
			pushToast({
				variant: "error",
				message: formatErrorMessage(error),
			});
		},
	});

	const reorderMutation = useMutation({
		mutationFn: async (domainIds: string[]) => {
			if (adminToken.length === 0) throw new Error("Missing admin token.");
			await reorderAdminRealityDomains(adminToken, domainIds);
		},
		onError: (error) => {
			pushToast({
				variant: "error",
				message: formatErrorMessage(error),
			});
		},
	});

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to manage global REALITY domains."
			/>
		);
	}

	if (nodesQuery.isLoading || domainsQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading settings"
				description="Fetching nodes and reality domains."
			/>
		);
	}

	if (nodesQuery.isError) {
		return (
			<PageState
				variant="error"
				title="Failed to load nodes"
				description={formatErrorMessage(nodesQuery.error)}
				action={
					<Button variant="secondary" onClick={() => nodesQuery.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	}

	if (domainsQuery.isError) {
		return (
			<PageState
				variant="error"
				title="Failed to load reality domains"
				description={formatErrorMessage(domainsQuery.error)}
				action={
					<Button variant="secondary" onClick={() => domainsQuery.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title="Reality domains"
				description="Global REALITY camouflage domain registry (shared across the cluster)."
				actions={
					<Button
						variant="secondary"
						loading={domainsQuery.isFetching || nodesQuery.isFetching}
						onClick={() => {
							nodesQuery.refetch();
							domainsQuery.refetch();
						}}
					>
						Refresh
					</Button>
				}
			/>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<h2 className="card-title">Add domains</h2>
					<TagInput
						label="serverNames"
						value={createServerNames}
						onChange={setCreateServerNames}
						placeholder="oneclient.sfx.ms"
						disabled={createMutation.isPending}
						inputClass={inputClass}
						validateTag={validateRealityServerName}
						helperText="Paste or type one or more hostnames. Order matters: the first enabled domain becomes primary for global endpoints."
					/>

					<div className="card-actions justify-end">
						<Button
							loading={createMutation.isPending}
							disabled={
								createMutation.isPending || createServerNames.length === 0
							}
							onClick={() => createMutation.mutate(createServerNames)}
						>
							Add
						</Button>
					</div>
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<div className="flex items-start justify-between gap-3">
						<div>
							<h2 className="card-title">Domain list</h2>
							<p className="text-sm opacity-70">
								{domains.length} domain{domains.length === 1 ? "" : "s"} total.
								Click node chips to enable/disable a domain per node.
							</p>
						</div>
						<Button
							variant="secondary"
							loading={domainsQuery.isFetching}
							onClick={() => domainsQuery.refetch()}
						>
							Refresh
						</Button>
					</div>

					{domains.length === 0 ? (
						<PageState
							variant="empty"
							title="No domains yet"
							description="Add at least one domain to use serverNamesSource=global."
						/>
					) : (
						<div className="overflow-x-auto">
							<table className="table table-zebra">
								<thead>
									<tr>
										<th className="w-20">Order</th>
										<th>serverName</th>
										<th>Nodes</th>
										<th className="w-24 text-right">Actions</th>
									</tr>
								</thead>
								<tbody>
									{domains.map((domain, idx) => {
										const disabled = domain.disabled_node_ids ?? [];
										const canMoveUp = idx > 0;
										const canMoveDown = idx < domains.length - 1;
										return (
											<tr key={domain.domain_id}>
												<td>
													<div className="flex items-center gap-1">
														<button
															type="button"
															className="btn btn-ghost btn-xs btn-square"
															disabled={reorderMutation.isPending || !canMoveUp}
															onClick={() => {
																const next = moveItem(domains, idx, idx - 1);
																queryClient.setQueryData(
																	["adminRealityDomains", adminToken],
																	{ items: next },
																);
																reorderMutation.mutate(
																	next.map((d) => d.domain_id),
																);
															}}
															title="Move up"
														>
															<Icon
																name="tabler:chevron-up"
																size={16}
																ariaLabel="Move up"
															/>
														</button>
														<button
															type="button"
															className="btn btn-ghost btn-xs btn-square"
															disabled={
																reorderMutation.isPending || !canMoveDown
															}
															onClick={() => {
																const next = moveItem(domains, idx, idx + 1);
																queryClient.setQueryData(
																	["adminRealityDomains", adminToken],
																	{ items: next },
																);
																reorderMutation.mutate(
																	next.map((d) => d.domain_id),
																);
															}}
															title="Move down"
														>
															<Icon
																name="tabler:chevron-down"
																size={16}
																ariaLabel="Move down"
															/>
														</button>
														<span className="text-xs opacity-60">
															{idx + 1}
														</span>
													</div>
												</td>
												<td className="font-mono text-sm">
													{domain.server_name}
												</td>
												<td>
													<div className="flex flex-wrap gap-2">
														{nodes.map((node) => {
															const isDisabled = disabled.includes(
																node.node_id,
															);
															return (
																<button
																	key={node.node_id}
																	type="button"
																	className={[
																		"badge gap-2",
																		isDisabled
																			? "badge-ghost opacity-60"
																			: "badge-primary",
																		patchMutation.isPending
																			? "cursor-wait"
																			: null,
																	]
																		.filter(Boolean)
																		.join(" ")}
																	disabled={patchMutation.isPending}
																	title={
																		isDisabled
																			? "Disabled on this node (click to enable)"
																			: "Enabled on this node (click to disable)"
																	}
																	onClick={() => {
																		const nextDisabled = toggleNodeId(
																			disabled,
																			node.node_id,
																		);
																		patchMutation.mutate({
																			domainId: domain.domain_id,
																			disabledNodeIds: nextDisabled,
																		});
																	}}
																>
																	<span>{nodeLabel(node)}</span>
																	<span className="font-mono text-xs opacity-70">
																		{node.node_id}
																	</span>
																</button>
															);
														})}
													</div>
												</td>
												<td className="text-right">
													<button
														type="button"
														className="btn btn-ghost btn-xs btn-square"
														disabled={deleteMutation.isPending}
														onClick={() =>
															setDeleteTarget({
																domainId: domain.domain_id,
																serverName: domain.server_name,
															})
														}
														title="Delete"
													>
														<Icon
															name="tabler:trash"
															size={16}
															ariaLabel="Delete"
														/>
													</button>
												</td>
											</tr>
										);
									})}
								</tbody>
							</table>
						</div>
					)}
				</div>
			</div>

			<ConfirmDialog
				open={deleteTarget !== null}
				title="Delete domain"
				description={
					deleteTarget
						? `Delete ${deleteTarget.serverName}? This may break global endpoints on nodes where it is the only enabled domain.`
						: ""
				}
				confirmLabel={deleteMutation.isPending ? "Deleting..." : "Delete"}
				onCancel={() => setDeleteTarget(null)}
				onConfirm={() => {
					if (!deleteTarget) return;
					const domainId = deleteTarget.domainId;
					setDeleteTarget(null);
					deleteMutation.mutate(domainId);
				}}
			/>
		</div>
	);
}
