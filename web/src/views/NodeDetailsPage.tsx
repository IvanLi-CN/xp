import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import {
	type AdminNodePatchRequest,
	deleteAdminNode,
	fetchAdminNode,
	fetchAdminNodeQuotaStatus,
	patchAdminNode,
	patchAdminNodeQuotaUsage,
} from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import type { NodeQuotaReset } from "../api/quotaReset";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";
import {
	type QuotaParseResult,
	formatQuotaBytesCompactInput,
	formatQuotaBytesHuman,
	parseQuotaInputToBytes,
} from "../utils/quota";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	return String(error);
}

export function NodeDetailsPage() {
	const { nodeId } = useParams({ from: "/app/nodes/$nodeId" });
	const [adminToken] = useState(() => readAdminToken());
	const { pushToast } = useToast();
	const prefs = useUiPrefs();
	const navigate = useNavigate();

	const nodeQuery = useQuery({
		queryKey: ["adminNode", adminToken, nodeId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNode(adminToken, nodeId, signal),
	});

	const quotaStatusQuery = useQuery({
		queryKey: ["adminNodeQuotaStatus", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodeQuotaStatus(adminToken, signal),
	});

	const quotaStatusItem = useMemo(() => {
		if (!quotaStatusQuery.data) return null;
		return (
			quotaStatusQuery.data.items.find((i) => i.node_id === nodeId) ?? null
		);
	}, [nodeId, quotaStatusQuery.data]);

	const [trafficMode, setTrafficMode] = useState<"unlimited" | "monthly_limit">(
		"unlimited",
	);
	const [limitDraft, setLimitDraft] = useState("");

	const [resetPolicy, setResetPolicy] = useState<"monthly" | "unlimited">(
		"monthly",
	);
	const [resetDay, setResetDay] = useState(1);
	const [resetTzOffsetMinutes, setResetTzOffsetMinutes] = useState<string>("");
	const [saveError, setSaveError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);
	const [deleteOpen, setDeleteOpen] = useState(false);
	const [isDeleting, setIsDeleting] = useState(false);

	const [usedDraft, setUsedDraft] = useState("");
	const [syncBaseline, setSyncBaseline] = useState(true);
	const [overrideError, setOverrideError] = useState<string | null>(null);
	const [isOverriding, setIsOverriding] = useState(false);

	useEffect(() => {
		if (nodeQuery.data) {
			const q = nodeQuery.data.quota_reset;
			const trafficMode =
				nodeQuery.data.quota_limit_bytes === 0 ? "unlimited" : "monthly_limit";
			setTrafficMode(trafficMode);
			setLimitDraft(
				nodeQuery.data.quota_limit_bytes === 0
					? ""
					: formatQuotaBytesCompactInput(nodeQuery.data.quota_limit_bytes),
			);

			if (q.policy === "monthly") {
				setResetPolicy("monthly");
				setResetDay(q.day_of_month);
			} else {
				setResetPolicy("unlimited");
				setResetDay(1);
			}
			const tz =
				q.tz_offset_minutes === null || q.tz_offset_minutes === undefined
					? ""
					: String(q.tz_offset_minutes);
			setResetTzOffsetMinutes(tz);
			setSaveError(null);
			setOverrideError(null);
		}
	}, [nodeQuery.data]);

	const desiredQuotaReset: NodeQuotaReset = useMemo(() => {
		const raw = resetTzOffsetMinutes.trim();
		const tz =
			raw.length === 0
				? undefined
				: Number.isFinite(Number(raw))
					? Number(raw)
					: undefined;
		return resetPolicy === "monthly"
			? {
					policy: "monthly",
					day_of_month: resetDay,
					...(tz === undefined ? {} : { tz_offset_minutes: tz }),
				}
			: {
					policy: "unlimited",
					...(tz === undefined ? {} : { tz_offset_minutes: tz }),
				};
	}, [resetDay, resetPolicy, resetTzOffsetMinutes]);

	const desiredQuotaLimitBytes = useMemo<QuotaParseResult>(() => {
		if (trafficMode === "unlimited") return { ok: true, bytes: 0 as number };
		const parsed = parseQuotaInputToBytes(limitDraft);
		if (!parsed.ok) return parsed;
		if (parsed.bytes === 0) {
			return {
				ok: false,
				error: "Monthly limit must be greater than 0.",
			} as const;
		}
		return parsed;
	}, [limitDraft, trafficMode]);

	const isDirty = useMemo(() => {
		if (!nodeQuery.data) return false;
		const q = nodeQuery.data.quota_reset;
		const currentPolicy = q.policy;
		const currentDay = currentPolicy === "monthly" ? q.day_of_month : 1;
		const currentTz =
			q.tz_offset_minutes === null || q.tz_offset_minutes === undefined
				? ""
				: String(q.tz_offset_minutes);
		const quotaResetDirty =
			resetPolicy !== currentPolicy ||
			(resetPolicy === "monthly" && resetDay !== currentDay) ||
			resetTzOffsetMinutes.trim() !== currentTz;

		const desiredLimitBytes =
			desiredQuotaLimitBytes.ok === true ? desiredQuotaLimitBytes.bytes : null;
		const quotaLimitDirty =
			desiredLimitBytes === null
				? true
				: desiredLimitBytes !== nodeQuery.data.quota_limit_bytes;

		return quotaResetDirty || quotaLimitDirty;
	}, [
		nodeQuery.data,
		resetDay,
		resetPolicy,
		resetTzOffsetMinutes,
		desiredQuotaLimitBytes,
	]);

	const handleSaveNodeTraffic = async () => {
		if (!nodeQuery.data) return;
		if (!isDirty) {
			pushToast({ variant: "info", message: "No changes to save." });
			return;
		}

		if (!desiredQuotaLimitBytes.ok) {
			setSaveError(desiredQuotaLimitBytes.error);
			return;
		}

		if (
			trafficMode === "monthly_limit" &&
			(resetPolicy !== "monthly" || desiredQuotaReset.policy !== "monthly")
		) {
			setSaveError(
				'Monthly limit requires quota_reset.policy="monthly" (pick monthly first).',
			);
			return;
		}

		if (
			resetPolicy === "monthly" &&
			(!Number.isInteger(resetDay) || resetDay < 1 || resetDay > 31)
		) {
			setSaveError("Reset day must be an integer between 1 and 31.");
			return;
		}

		const tzRaw = resetTzOffsetMinutes.trim();
		if (trafficMode === "monthly_limit" && tzRaw.length === 0) {
			setSaveError("tz_offset_minutes is required for monthly node limit.");
			return;
		}
		if (tzRaw.length > 0) {
			const tz = Number(tzRaw);
			if (!Number.isFinite(tz) || !Number.isInteger(tz)) {
				setSaveError("tz_offset_minutes must be an integer (or empty).");
				return;
			}
		}

		setIsSaving(true);
		setSaveError(null);

		const payload: AdminNodePatchRequest = {
			quota_reset: desiredQuotaReset,
			quota_limit_bytes: desiredQuotaLimitBytes.bytes,
		};

		try {
			await patchAdminNode(adminToken, nodeId, payload);
			pushToast({ variant: "success", message: "Node updated." });
			await Promise.all([nodeQuery.refetch(), quotaStatusQuery.refetch()]);
		} catch (error) {
			const message = formatErrorMessage(error);
			setSaveError(message);
			pushToast({
				variant: "error",
				message: "Failed to update node.",
			});
		} finally {
			setIsSaving(false);
		}
	};

	const handleOverrideUsedBytes = async () => {
		if (!nodeQuery.data) return;

		const parsed = parseQuotaInputToBytes(usedDraft);
		if (!parsed.ok) {
			setOverrideError(parsed.error);
			return;
		}

		const tzRaw = resetTzOffsetMinutes.trim();
		if (resetPolicy !== "monthly" || tzRaw.length === 0) {
			setOverrideError(
				"Usage override requires quota_reset.policy=monthly and tz_offset_minutes set.",
			);
			return;
		}

		setIsOverriding(true);
		setOverrideError(null);
		try {
			const res = await patchAdminNodeQuotaUsage(adminToken, nodeId, {
				used_bytes: parsed.bytes,
				sync_baseline: syncBaseline,
			});
			const warning = res.warning ? ` (${res.warning})` : "";
			pushToast({
				variant: "success",
				message: `Usage updated.${warning}`,
			});
			await quotaStatusQuery.refetch();
		} catch (error) {
			const message = formatErrorMessage(error);
			setOverrideError(message);
			pushToast({
				variant: "error",
				message: "Failed to update usage.",
			});
		} finally {
			setIsOverriding(false);
		}
	};

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Please provide an admin token to load node details."
				/>
			);
		}

		if (nodeQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading node"
					description="Fetching node metadata."
				/>
			);
		}

		if (nodeQuery.isError) {
			return (
				<PageState
					variant="error"
					title="Failed to load node"
					description={formatErrorMessage(nodeQuery.error)}
					action={
						<Button
							variant="secondary"
							loading={nodeQuery.isFetching}
							onClick={() => nodeQuery.refetch()}
						>
							Retry
						</Button>
					}
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

		const inputClass =
			prefs.density === "compact"
				? "input input-bordered input-sm"
				: "input input-bordered";
		const selectClass =
			prefs.density === "compact"
				? "select select-bordered select-sm"
				: "select select-bordered";

		return (
			<div className="space-y-4">
				<div className="card bg-base-100 shadow">
					<div className="card-body space-y-3">
						<div>
							<h2 className="card-title">Node metadata</h2>
							<p className="text-sm opacity-70">
								Read-only. Managed via xp-ops config file.
							</p>
						</div>
						<div className="rounded-box bg-base-200 p-4 space-y-2">
							<div className="text-xs uppercase tracking-wide opacity-60">
								Node ID
							</div>
							<div className="font-mono text-sm break-all">{nodeId}</div>
							<div className="grid gap-2 md:grid-cols-2 pt-3">
								<div>
									<div className="text-xs uppercase tracking-wide opacity-60">
										Node name
									</div>
									<div className="font-mono text-sm break-all">
										{nodeQuery.data.node_name || "(empty)"}
									</div>
								</div>
								<div>
									<div className="text-xs uppercase tracking-wide opacity-60">
										Access host
									</div>
									<div className="font-mono text-sm break-all">
										{nodeQuery.data.access_host || "(empty)"}
									</div>
								</div>
								<div className="md:col-span-2">
									<div className="text-xs uppercase tracking-wide opacity-60">
										API base URL
									</div>
									<div className="font-mono text-sm break-all">
										{nodeQuery.data.api_base_url || "(empty)"}
									</div>
								</div>
							</div>
						</div>
						<div className="text-sm opacity-70">
							To change node meta, edit{" "}
							<span className="font-mono">/etc/xp/xp.env</span> and run{" "}
							<span className="font-mono">sudo xp-ops xp sync-node-meta</span>.
						</div>
					</div>
				</div>

				<div className="card bg-base-100 shadow">
					<div className="card-body space-y-4">
						<div>
							<h2 className="card-title">Node traffic</h2>
							<p className="text-sm opacity-70">
								Runtime admin setting. Unlimited or monthly limit. Quota
								enforcement never mutates desired state.
							</p>
						</div>
						<div className="grid gap-4 md:grid-cols-3">
							<label className="form-control">
								<div className="label">
									<span className="label-text">Mode</span>
								</div>
								<select
									className={selectClass}
									value={trafficMode}
									onChange={(e) => {
										const v = e.target.value as "unlimited" | "monthly_limit";
										setTrafficMode(v);
										if (v === "monthly_limit") {
											setResetPolicy("monthly");
										}
									}}
								>
									<option value="unlimited">unlimited</option>
									<option value="monthly_limit">monthly limit</option>
								</select>
							</label>
							<label className="form-control md:col-span-2">
								<div className="label">
									<span className="label-text">Monthly limit</span>
								</div>
								<input
									className={inputClass}
									type="text"
									disabled={trafficMode !== "monthly_limit"}
									value={limitDraft}
									onChange={(e) => setLimitDraft(e.target.value)}
									placeholder="e.g. 2TiB"
								/>
								<div className="label">
									<span className="label-text-alt opacity-70">
										Supports MiB/GiB/TiB/PiB; GB/TB/PB are accepted as
										GiB/TiB/PiB.
									</span>
								</div>
							</label>
						</div>

						<div className="grid gap-4 md:grid-cols-3">
							<label className="form-control">
								<div className="label">
									<span className="label-text">Policy</span>
								</div>
								<select
									className={selectClass}
									value={resetPolicy}
									onChange={(e) =>
										setResetPolicy(e.target.value as "monthly" | "unlimited")
									}
									disabled={trafficMode === "monthly_limit"}
								>
									<option value="monthly">monthly</option>
									<option value="unlimited">unlimited</option>
								</select>
							</label>
							<label className="form-control">
								<div className="label">
									<span className="label-text">Day of month</span>
								</div>
								<input
									className={inputClass}
									type="number"
									min={1}
									max={31}
									step={1}
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
									type="text"
									value={resetTzOffsetMinutes}
									onChange={(e) => setResetTzOffsetMinutes(e.target.value)}
									placeholder="(empty)"
								/>
								{trafficMode === "monthly_limit" ? (
									<div className="label">
										<span className="label-text-alt text-warning">
											Required for monthly node limit.
										</span>
									</div>
								) : null}
							</label>
						</div>

						{saveError ? (
							<p className="text-sm text-error">{saveError}</p>
						) : null}

						<div className="flex justify-end gap-2">
							<Button
								variant="secondary"
								loading={nodeQuery.isFetching}
								onClick={() => nodeQuery.refetch()}
							>
								Refresh
							</Button>
							<Button
								variant="primary"
								loading={isSaving}
								disabled={!isDirty}
								onClick={handleSaveNodeTraffic}
							>
								Save changes
							</Button>
						</div>
					</div>
				</div>

				<div className="card bg-base-100 shadow">
					<div className="card-body space-y-4">
						<div>
							<h2 className="card-title">Usage override (IDC align)</h2>
							<p className="text-sm opacity-70">
								Set absolute used bytes for the current node cycle. This will
								immediately re-evaluate exhaustion and trigger reconcile.
							</p>
						</div>

						{quotaStatusItem ? (
							<div className="rounded-box bg-base-200 p-4 space-y-2">
								<div className="grid gap-2 md:grid-cols-2">
									<div>
										<div className="text-xs uppercase tracking-wide opacity-60">
											Limit
										</div>
										<div className="font-mono text-sm break-all">
											{quotaStatusItem.quota_limit_bytes === 0
												? "unlimited"
												: formatQuotaBytesHuman(
														quotaStatusItem.quota_limit_bytes,
													)}
										</div>
									</div>
									<div>
										<div className="text-xs uppercase tracking-wide opacity-60">
											Used
										</div>
										<div className="font-mono text-sm break-all">
											{formatQuotaBytesHuman(quotaStatusItem.used_bytes)}
										</div>
									</div>
									<div>
										<div className="text-xs uppercase tracking-wide opacity-60">
											Remaining
										</div>
										<div className="font-mono text-sm break-all">
											{quotaStatusItem.quota_limit_bytes === 0
												? "-"
												: formatQuotaBytesHuman(
														quotaStatusItem.remaining_bytes,
													)}
										</div>
									</div>
									<div>
										<div className="text-xs uppercase tracking-wide opacity-60">
											Next reset
										</div>
										<div className="font-mono text-sm break-all">
											{quotaStatusItem.cycle_end_at ?? "-"}
										</div>
									</div>
								</div>
								<div className="flex items-center gap-2 pt-2">
									{quotaStatusItem.exhausted ? (
										<span className="badge badge-error">exhausted</span>
									) : (
										<span className="badge badge-success">ok</span>
									)}
									{quotaStatusItem.warning ? (
										<span className="badge badge-warning">
											{quotaStatusItem.warning}
										</span>
									) : null}
								</div>
							</div>
						) : quotaStatusQuery.isFetching ? (
							<p className="text-sm opacity-70">Loading quota status...</p>
						) : (
							<p className="text-sm text-warning">
								Quota status is unavailable for this node.
							</p>
						)}

						<div className="grid gap-4 md:grid-cols-3">
							<label className="form-control md:col-span-2">
								<div className="label">
									<span className="label-text">Set used</span>
								</div>
								<input
									className={inputClass}
									type="text"
									value={usedDraft}
									onChange={(e) => setUsedDraft(e.target.value)}
									placeholder="e.g. 123GiB"
								/>
							</label>
							<label className="form-control">
								<div className="label">
									<span className="label-text">sync_baseline</span>
								</div>
								<input
									className="toggle toggle-primary"
									type="checkbox"
									checked={syncBaseline}
									onChange={(e) => setSyncBaseline(e.target.checked)}
								/>
								<div className="label">
									<span className="label-text-alt opacity-70">
										Align inbound baselines.
									</span>
								</div>
							</label>
						</div>

						{overrideError ? (
							<p className="text-sm text-error">{overrideError}</p>
						) : null}

						<div className="flex justify-end gap-2">
							<Button
								variant="secondary"
								loading={quotaStatusQuery.isFetching}
								onClick={() => quotaStatusQuery.refetch()}
							>
								Refresh
							</Button>
							<Button
								variant="primary"
								loading={isOverriding}
								disabled={usedDraft.trim().length === 0}
								onClick={handleOverrideUsedBytes}
							>
								Apply override
							</Button>
						</div>
					</div>
				</div>

				<div className="card bg-base-100 shadow border border-error/30">
					<div className="card-body space-y-4">
						<h2 className="card-title text-error">Danger zone</h2>
						<p className="text-sm opacity-70">
							Deleting a node removes it from the cluster membership and
							inventory. This action cannot be undone. Only delete nodes that
							have no endpoints.
						</p>
						<div>
							<Button
								variant="danger"
								onClick={() => setDeleteOpen(true)}
								disabled={isDeleting}
							>
								Delete node
							</Button>
						</div>
					</div>
				</div>

				<ConfirmDialog
					open={deleteOpen}
					title="Delete node?"
					description="This action cannot be undone. The node must have no endpoints."
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
										await deleteAdminNode(adminToken, nodeId);
										pushToast({
											variant: "success",
											message: "Node deleted.",
										});
										navigate({ to: "/nodes" });
									} catch (error) {
										pushToast({
											variant: "error",
											message: formatErrorMessage(error),
										});
									} finally {
										setIsDeleting(false);
										setDeleteOpen(false);
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
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="Node details"
				description="Manage node metadata and routing configuration."
				actions={
					<Link to="/nodes" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>
			{content}
		</div>
	);
}
