import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import { cn } from "@/lib/utils";

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
import { TableCell } from "../components/DataTable";
import { Icon } from "../components/Icon";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import {
	ResourceTable,
	type ResourceTableHeader,
} from "../components/ResourceTable";
import { TagInput } from "../components/TagInput";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { badgeClass } from "../components/ui-helpers";
import {
	Form,
	FormControl,
	FormDescription,
	FormField,
	FormItem,
	FormMessage,
} from "../components/ui/form";
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

const createDomainsSchema = z.object({
	serverNames: z
		.array(z.string())
		.min(1, "Add at least one domain.")
		.superRefine((values, ctx) => {
			for (const [index, value] of values.entries()) {
				const normalized = normalizeRealityServerName(value);
				const error = validateRealityServerName(normalized);
				if (error) {
					ctx.addIssue({
						code: z.ZodIssueCode.custom,
						path: [index],
						message: error,
					});
				}
			}
		}),
});

type CreateDomainsValues = z.infer<typeof createDomainsSchema>;

const tableHeaders: ResourceTableHeader[] = [
	{ key: "order", label: "Order", className: "w-24" },
	{ key: "serverName", label: "serverName" },
	{ key: "nodes", label: "Nodes" },
	{ key: "actions", label: "Actions", align: "right", className: "w-24" },
];

export function RealityDomainsPage() {
	const queryClient = useQueryClient();
	const { pushToast } = useToast();
	const adminToken = readAdminToken();
	const createForm = useForm<CreateDomainsValues>({
		resolver: zodResolver(createDomainsSchema),
		defaultValues: {
			serverNames: [],
		},
	});

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
			createForm.reset({ serverNames: [] });
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
			pushToast({ variant: "error", message: formatErrorMessage(error) });
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
			pushToast({ variant: "error", message: formatErrorMessage(error) });
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
			pushToast({ variant: "error", message: formatErrorMessage(error) });
		},
	});

	const reorderMutation = useMutation({
		mutationFn: async (orderedIds: string[]) => {
			if (adminToken.length === 0) throw new Error("Missing admin token.");
			return reorderAdminRealityDomains(adminToken, orderedIds);
		},
		onSuccess: () => {
			void domainsQuery.refetch();
		},
		onError: (error) => {
			void domainsQuery.refetch();
			pushToast({ variant: "error", message: formatErrorMessage(error) });
		},
	});

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Please provide an admin token to manage reality domains."
			/>
		);
	}

	if (nodesQuery.isLoading || domainsQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading reality domains"
				description="Fetching nodes and domain registry data."
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
							void nodesQuery.refetch();
							void domainsQuery.refetch();
						}}
					>
						Refresh
					</Button>
				}
			/>

			<div className="xp-card">
				<div className="xp-card-body space-y-4">
					<h2 className="xp-card-title">Add domains</h2>
					<Form {...createForm}>
						<form
							className="space-y-4"
							onSubmit={createForm.handleSubmit((values) =>
								createMutation.mutate(values.serverNames),
							)}
						>
							<FormField
								control={createForm.control}
								name="serverNames"
								render={({ field }) => (
									<FormItem>
										<FormControl>
											<TagInput
												label="serverNames"
												value={field.value}
												onChange={field.onChange}
												placeholder="download.example.com"
												disabled={createMutation.isPending}
												validateTag={validateRealityServerName}
												helperText="Paste or type one or more hostnames. Order matters: the first enabled domain becomes primary for global endpoints."
											/>
										</FormControl>
										<FormDescription>
											Paste or type one or more hostnames. Order matters: the
											first enabled domain becomes primary for global endpoints.
										</FormDescription>
										<FormMessage />
									</FormItem>
								)}
							/>

							<div className="flex justify-end">
								<Button
									type="submit"
									loading={createMutation.isPending}
									disabled={
										createMutation.isPending ||
										createForm.watch("serverNames").length === 0
									}
								>
									Add
								</Button>
							</div>
						</form>
					</Form>
				</div>
			</div>

			<div className="xp-card">
				<div className="xp-card-body space-y-4">
					<div className="flex items-start justify-between gap-3">
						<div>
							<h2 className="xp-card-title">Domain list</h2>
							<p className="text-sm text-muted-foreground">
								{domains.length} domain{domains.length === 1 ? "" : "s"} total.
								Click node chips to enable or disable a domain per node.
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
						<ResourceTable headers={tableHeaders}>
							{domains.map((domain, idx) => {
								const disabled = domain.disabled_node_ids ?? [];
								const canMoveUp = idx > 0;
								const canMoveDown = idx < domains.length - 1;
								return (
									<tr key={domain.domain_id}>
										<TableCell>
											<div className="flex items-center gap-1">
												<Button
													variant="ghost"
													size="sm"
													className="size-7 p-0"
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
												</Button>
												<Button
													variant="ghost"
													size="sm"
													className="size-7 p-0"
													disabled={reorderMutation.isPending || !canMoveDown}
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
												</Button>
												<span className="text-xs text-muted-foreground">
													{idx + 1}
												</span>
											</div>
										</TableCell>
										<TableCell className="font-mono text-sm">
											{domain.server_name}
										</TableCell>
										<TableCell>
											<div className="flex flex-wrap gap-2">
												{nodes.map((node) => {
													const isDisabled = disabled.includes(node.node_id);
													return (
														<button
															key={node.node_id}
															type="button"
															className={cn(
																badgeClass(
																	isDisabled ? "ghost" : "primary",
																	"default",
																	"gap-2 transition-opacity",
																),
																isDisabled && "opacity-60",
																patchMutation.isPending && "cursor-wait",
															)}
															disabled={patchMutation.isPending}
															title={
																isDisabled
																	? "Disabled on this node (click to enable)"
																	: "Enabled on this node (click to disable)"
															}
															onClick={() => {
																patchMutation.mutate({
																	domainId: domain.domain_id,
																	disabledNodeIds: toggleNodeId(
																		disabled,
																		node.node_id,
																	),
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
										</TableCell>
										<TableCell className="text-right">
											<Button
												variant="ghost"
												size="sm"
												className="size-7 p-0"
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
											</Button>
										</TableCell>
									</tr>
								);
							})}
						</ResourceTable>
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
