import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import {
	type AdminNodePatchRequest,
	fetchAdminNode,
	patchAdminNode,
} from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import type { NodeQuotaReset } from "../api/quotaReset";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";

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

	const nodeQuery = useQuery({
		queryKey: ["adminNode", adminToken, nodeId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNode(adminToken, nodeId, signal),
	});

	const [resetPolicy, setResetPolicy] = useState<"monthly" | "unlimited">(
		"monthly",
	);
	const [resetDay, setResetDay] = useState(1);
	const [resetTzOffsetMinutes, setResetTzOffsetMinutes] = useState<string>("");
	const [saveError, setSaveError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);

	useEffect(() => {
		if (nodeQuery.data) {
			const q = nodeQuery.data.quota_reset;
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

	const isDirty = useMemo(() => {
		if (!nodeQuery.data) return false;
		const q = nodeQuery.data.quota_reset;
		const currentPolicy = q.policy;
		const currentDay = currentPolicy === "monthly" ? q.day_of_month : 1;
		const currentTz =
			q.tz_offset_minutes === null || q.tz_offset_minutes === undefined
				? ""
				: String(q.tz_offset_minutes);
		return (
			resetPolicy !== currentPolicy ||
			(resetPolicy === "monthly" && resetDay !== currentDay) ||
			resetTzOffsetMinutes.trim() !== currentTz
		);
	}, [nodeQuery.data, resetDay, resetPolicy, resetTzOffsetMinutes]);

	const handleSaveQuotaReset = async () => {
		if (!nodeQuery.data) return;
		if (!isDirty) {
			pushToast({ variant: "info", message: "No changes to save." });
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
		};

		try {
			await patchAdminNode(adminToken, nodeId, payload);
			pushToast({ variant: "success", message: "Node updated." });
			await nodeQuery.refetch();
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
							<h2 className="card-title">Quota reset</h2>
							<p className="text-sm opacity-70">
								Runtime admin setting. Safe to edit via the admin API.
							</p>
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
								onClick={handleSaveQuotaReset}
							>
								Save changes
							</Button>
						</div>
					</div>
				</div>
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
