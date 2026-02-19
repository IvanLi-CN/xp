import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import { fetchAdminNodes, patchAdminNode } from "../api/adminNodes";
import {
	fetchAdminUserNodeWeights,
	putAdminUserNodeWeight,
} from "../api/adminUserNodeWeights";
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

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

function clampU16Weight(
	raw: string,
): { ok: true; weight: number } | { ok: false; error: string } {
	const trimmed = raw.trim();
	if (trimmed.length === 0) {
		return { ok: true, weight: 100 };
	}
	const n = Number(trimmed);
	if (!Number.isFinite(n) || !Number.isInteger(n)) {
		return { ok: false, error: "Weight must be an integer." };
	}
	if (n < 0 || n > 65535) {
		return { ok: false, error: "Weight must be between 0 and 65535." };
	}
	return { ok: true, weight: n };
}

function formatNodeQuotaResetBrief(q: {
	policy: "monthly" | "unlimited";
	day_of_month?: number;
	tz_offset_minutes?: number | null;
}): string {
	const tz =
		q.tz_offset_minutes === null || q.tz_offset_minutes === undefined
			? "(local)"
			: `UTC${q.tz_offset_minutes >= 0 ? "+" : ""}${q.tz_offset_minutes}`;
	if (q.policy === "monthly") {
		return `monthly@${q.day_of_month ?? 1} ${tz}`;
	}
	return `unlimited ${tz}`;
}

export function QuotaPolicyPage() {
	const adminToken = readAdminToken();
	const { pushToast } = useToast();
	const prefs = useUiPrefs();

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

	const nodes = nodesQuery.data?.items ?? [];
	const users = usersQuery.data?.items ?? [];

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";

	// ---- Node weights modal ----
	const [weightsOpen, setWeightsOpen] = useState(false);
	const [weightsUserId, setWeightsUserId] = useState<string | null>(null);
	const [weightsDraft, setWeightsDraft] = useState<Record<string, string>>({});
	const [weightsError, setWeightsError] = useState<string | null>(null);
	const [isSavingWeights, setIsSavingWeights] = useState(false);

	const weightsUser = useMemo(() => {
		if (!weightsUserId) return null;
		return users.find((u) => u.user_id === weightsUserId) ?? null;
	}, [users, weightsUserId]);

	const weightsQuery = useQuery({
		queryKey: ["adminUserNodeWeights", adminToken, weightsUserId],
		enabled: adminToken.length > 0 && Boolean(weightsUserId) && weightsOpen,
		queryFn: ({ signal }) =>
			fetchAdminUserNodeWeights(adminToken, weightsUserId ?? "", signal),
	});

	useEffect(() => {
		if (!weightsOpen || !weightsUserId) return;

		const existing = new Map(
			(weightsQuery.data?.items ?? []).map((i) => [i.node_id, i.weight]),
		);
		const next: Record<string, string> = {};
		for (const node of nodes) {
			const w = existing.get(node.node_id) ?? 100;
			next[node.node_id] = String(w);
		}
		setWeightsDraft(next);
		setWeightsError(null);
	}, [nodes, weightsOpen, weightsQuery.data, weightsUserId]);

	const saveWeights = async () => {
		if (!weightsUserId) return;
		if (nodes.length === 0) return;

		setWeightsError(null);
		setIsSavingWeights(true);
		try {
			const existing = new Map(
				(weightsQuery.data?.items ?? []).map((i) => [i.node_id, i.weight]),
			);

			const updates: Array<{ nodeId: string; weight: number }> = [];
			for (const node of nodes) {
				const parsed = clampU16Weight(weightsDraft[node.node_id] ?? "");
				if (!parsed.ok) {
					setWeightsError(`${node.node_name}: ${parsed.error}`);
					return;
				}
				const current = existing.get(node.node_id) ?? 100;
				if (parsed.weight !== current) {
					updates.push({ nodeId: node.node_id, weight: parsed.weight });
				}
			}

			for (const u of updates) {
				await putAdminUserNodeWeight(
					adminToken,
					weightsUserId,
					u.nodeId,
					u.weight,
				);
			}

			pushToast({
				variant: "success",
				message:
					updates.length === 0
						? "No weight changes to save."
						: "Weights updated.",
			});

			await weightsQuery.refetch();
			setWeightsOpen(false);
			setWeightsUserId(null);
		} catch (err) {
			const message = formatError(err);
			setWeightsError(message);
			pushToast({
				variant: "error",
				message: `Failed to update weights: ${message}`,
			});
		} finally {
			setIsSavingWeights(false);
		}
	};

	const weightsModal = (
		<dialog className="modal" open={weightsOpen}>
			<div className="modal-box max-w-3xl">
				<div className="space-y-2">
					<h3 className="text-lg font-bold">Edit node weights</h3>
					<p className="text-sm opacity-70">
						Weights are scoped per user per node. Default is{" "}
						<span className="font-mono">100</span>. Users cannot query their own
						weights.
					</p>
					{weightsUser ? (
						<p className="text-xs opacity-70">
							User:{" "}
							<span className="font-mono">
								{weightsUser.user_id} — {weightsUser.display_name}
							</span>
						</p>
					) : null}
				</div>

				<div className="py-4">
					{nodesQuery.isLoading || weightsQuery.isLoading ? (
						<div className="text-sm opacity-70">Loading...</div>
					) : nodesQuery.isError ? (
						<div className="text-sm text-error">
							{formatError(nodesQuery.error)}
						</div>
					) : weightsQuery.isError ? (
						<div className="text-sm text-error">
							{formatError(weightsQuery.error)}
						</div>
					) : nodes.length === 0 ? (
						<div className="text-sm opacity-70">No nodes.</div>
					) : (
						<div className="overflow-auto rounded-box border border-base-200">
							<table className="table table-fixed w-full">
								<thead>
									<tr className="bg-base-200/50">
										<th className="w-60">Node</th>
										<th className="w-40">Weight</th>
										<th>Node ID</th>
									</tr>
								</thead>
								<tbody>
									{nodes.map((n) => (
										<tr key={n.node_id}>
											<td className="align-top">
												<div className="font-semibold">{n.node_name}</div>
												<div className="text-xs opacity-60">
													{formatNodeQuotaResetBrief(n.quota_reset)}
												</div>
											</td>
											<td className="align-top">
												<input
													className={[inputClass, "font-mono"].join(" ")}
													value={weightsDraft[n.node_id] ?? ""}
													disabled={isSavingWeights}
													onChange={(e) =>
														setWeightsDraft((prev) => ({
															...prev,
															[n.node_id]: e.target.value,
														}))
													}
													placeholder="100"
													aria-label={`Weight for ${n.node_name}`}
												/>
											</td>
											<td className="align-top">
												<div className="font-mono text-xs break-all">
													{n.node_id}
												</div>
											</td>
										</tr>
									))}
								</tbody>
							</table>
						</div>
					)}

					{weightsError ? (
						<p className="mt-3 text-sm text-error">{weightsError}</p>
					) : null}
				</div>

				<div className="modal-action">
					<Button
						variant="secondary"
						disabled={isSavingWeights}
						onClick={() => {
							setWeightsOpen(false);
							setWeightsUserId(null);
							setWeightsError(null);
						}}
					>
						Cancel
					</Button>
					<Button
						variant="primary"
						loading={isSavingWeights}
						disabled={isSavingWeights}
						onClick={() => void saveWeights()}
					>
						Save
					</Button>
				</div>
			</div>
			<form method="dialog" className="modal-backdrop">
				<button
					type="button"
					onClick={() => {
						setWeightsOpen(false);
						setWeightsUserId(null);
						setWeightsError(null);
					}}
				>
					close
				</button>
			</form>
		</dialog>
	);

	// ---- Page content ----
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

	if (nodesQuery.isLoading || usersQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading quota policy"
				description="Fetching nodes and users."
			/>
		);
	}

	if (nodesQuery.isError || usersQuery.isError) {
		const message = nodesQuery.isError
			? formatError(nodesQuery.error)
			: usersQuery.isError
				? formatError(usersQuery.error)
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
						}}
					>
						Retry
					</Button>
				}
			/>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title="Quota policy"
				description="Shared node quota budgets, user tiers, and per-node weights."
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
					{nodes.map((n) => (
						<tr key={n.node_id}>
							<td className="align-top">
								<div className="flex flex-col gap-1 min-w-0">
									<Link
										to="/nodes/$nodeId"
										params={{ nodeId: n.node_id }}
										className="link link-hover font-semibold block truncate"
										title="Open node details"
									>
										{n.node_name}
									</Link>
									<div className="font-mono text-xs opacity-70 break-all">
										{n.node_id}
									</div>
								</div>
							</td>
							<td className="align-top">
								<NodeQuotaEditor
									value={n.quota_limit_bytes}
									onApply={async (nextBytes) => {
										try {
											await patchAdminNode(adminToken, n.node_id, {
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
									{formatNodeQuotaResetBrief(n.quota_reset)}
								</div>
							</td>
						</tr>
					))}
				</ResourceTable>
			</div>

			<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
				<div className="space-y-1">
					<h2 className="text-lg font-semibold">User tiers & weights</h2>
					<p className="text-sm opacity-70">
						Tier controls pacing behavior (P1 less restrictive, P2 balanced, P3
						opportunistic). Weights are node-scoped and admin-only.
					</p>
				</div>

				<ResourceTable
					tableClassName="table-fixed w-full"
					headers={[
						{ key: "user", label: "User" },
						{ key: "tier", label: "Tier" },
						{ key: "weights", label: "Node weights" },
					]}
				>
					{users.map((u) => (
						<tr key={u.user_id}>
							<td className="align-top">
								<div className="flex flex-col gap-1 min-w-0">
									<Link
										to="/users/$userId"
										params={{ userId: u.user_id }}
										className="link link-hover font-semibold block truncate"
										title="Open user details"
									>
										{u.display_name}
									</Link>
									<div className="font-mono text-xs opacity-70 break-all">
										{u.user_id}
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
									value={u.priority_tier}
									onChange={async (e) => {
										const next = e.target.value as "p1" | "p2" | "p3";
										try {
											await patchAdminUser(adminToken, u.user_id, {
												priority_tier: next,
											});
											pushToast({
												variant: "success",
												message: "User tier updated.",
											});
											await usersQuery.refetch();
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
							<td className="align-top">
								<Button
									variant="secondary"
									size="sm"
									onClick={() => {
										setWeightsUserId(u.user_id);
										setWeightsOpen(true);
									}}
								>
									Edit weights
								</Button>
							</td>
						</tr>
					))}
				</ResourceTable>
			</div>

			{weightsModal}
		</div>
	);
}
