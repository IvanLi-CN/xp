import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import {
	type AdminGrantPatchRequest,
	type CyclePolicy,
	deleteAdminGrant,
	fetchAdminGrant,
	fetchAdminGrantUsage,
	patchAdminGrant,
} from "../api/adminGrants";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { CopyButton } from "../components/CopyButton";
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

export function GrantDetailsPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
	const { grantId } = useParams({ from: "/app/grants/$grantId" });
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
			? "textarea textarea-bordered textarea-sm"
			: "textarea textarea-bordered";

	const grantQuery = useQuery({
		queryKey: ["adminGrant", adminToken, grantId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminGrant(adminToken, grantId, signal),
	});

	const usageQuery = useQuery({
		queryKey: ["adminGrantUsage", adminToken, grantId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminGrantUsage(adminToken, grantId, signal),
	});

	const [enabled, setEnabled] = useState(true);
	const [cyclePolicy, setCyclePolicy] = useState<CyclePolicy>("inherit_user");
	const [cycleDay, setCycleDay] = useState(1);
	const [note, setNote] = useState("");
	const [formError, setFormError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);
	const [disableOpen, setDisableOpen] = useState(false);
	const [deleteOpen, setDeleteOpen] = useState(false);
	const [isDeleting, setIsDeleting] = useState(false);

	useEffect(() => {
		if (!grantQuery.data) return;
		setEnabled(grantQuery.data.enabled);
		setCyclePolicy(grantQuery.data.cycle_policy);
		setCycleDay(grantQuery.data.cycle_day_of_month ?? 1);
		setNote(grantQuery.data.note ?? "");
	}, [grantQuery.data]);

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to load grant details."
				action={
					<Link to="/login" className="btn btn-primary">
						Go to login
					</Link>
				}
			/>
		);
	}

	if (grantQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading grant"
				description="Fetching grant details from the control plane."
			/>
		);
	}

	if (grantQuery.isError) {
		return (
			<PageState
				variant="error"
				title="Failed to load grant"
				description={formatError(grantQuery.error)}
				action={
					<Button variant="secondary" onClick={() => grantQuery.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	}

	if (!grantQuery.data) {
		return (
			<PageState
				variant="empty"
				title="Grant not found"
				description="The grant ID does not exist."
				action={
					<Link to="/grants" className="btn btn-outline btn-sm xp-btn-outline">
						Back to grants
					</Link>
				}
			/>
		);
	}

	const grant = grantQuery.data;
	const usageNotImplemented =
		usageQuery.isError &&
		isBackendApiError(usageQuery.error) &&
		usageQuery.error.status === 501;

	const handleSave = async (confirmedDisable: boolean) => {
		if (isSaving) return;
		if (cyclePolicy !== "inherit_user") {
			if (cycleDay < 1 || cycleDay > 31) {
				setFormError("Cycle day must be between 1 and 31.");
				return;
			}
		}

		if (!confirmedDisable && grant.enabled && !enabled) {
			setDisableOpen(true);
			return;
		}

		setFormError(null);
		setIsSaving(true);
		try {
			const payload: AdminGrantPatchRequest = {
				enabled,
				quota_limit_bytes: grant.quota_limit_bytes,
				cycle_policy: cyclePolicy,
				cycle_day_of_month: cyclePolicy === "inherit_user" ? null : cycleDay,
			};
			const currentNote = grant.note ?? null;
			const nextNote = note.trim() ? note.trim() : null;
			if (nextNote !== currentNote) {
				payload.note = nextNote;
			}
			const updated = await patchAdminGrant(
				adminToken,
				grant.grant_id,
				payload,
			);
			queryClient.setQueryData(["adminGrant", adminToken, grantId], updated);
			pushToast({
				variant: "success",
				message: "Grant updated.",
			});
		} catch (err) {
			setFormError(formatError(err));
			pushToast({
				variant: "error",
				message: "Failed to update grant.",
			});
		} finally {
			setIsSaving(false);
			setDisableOpen(false);
		}
	};

	return (
		<div className="space-y-6">
			<PageHeader
				title="Grant details"
				description={
					<>
						Grant ID: <span className="font-mono">{grant.grant_id}</span>
					</>
				}
				actions={
					<Link to="/grants" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>

			<form
				className="card bg-base-100 shadow"
				onSubmit={async (event) => {
					event.preventDefault();
					await handleSave(false);
				}}
			>
				<div className="card-body space-y-4">
					<h2 className="card-title">Grant settings</h2>
					<div className="flex flex-wrap items-center gap-4">
						<label className="flex items-center gap-2">
							<input
								type="checkbox"
								className="toggle"
								checked={enabled}
								onChange={(event) => setEnabled(event.target.checked)}
							/>
							<span>Enabled</span>
						</label>
						<p className="text-sm opacity-70">
							User: <span className="font-mono">{grant.user_id}</span>
						</p>
						<p className="text-sm opacity-70">
							Endpoint: <span className="font-mono">{grant.endpoint_id}</span>
						</p>
					</div>
					<div className="grid gap-4 md:grid-cols-2">
						<div className="form-control">
							<div className="label">
								<span className="label-text">Quota (node)</span>
							</div>
							<div className="flex flex-wrap items-baseline gap-3">
								<span className="font-mono text-sm">
									{grant.quota_limit_bytes} bytes
								</span>
								<Link
									to="/users/$userId"
									params={{ userId: grant.user_id }}
									className="link link-primary text-sm"
								>
									Edit node quota in user details
								</Link>
							</div>
						</div>
						<label className="form-control">
							<div className="label">
								<span className="label-text">Cycle policy</span>
							</div>
							<select
								className={selectClass}
								value={cyclePolicy}
								onChange={(event) => {
									const next = event.target.value as CyclePolicy;
									setCyclePolicy(next);
									if (next === "inherit_user") {
										setCycleDay(1);
									}
								}}
							>
								<option value="inherit_user">inherit_user</option>
								<option value="by_user">by_user</option>
								<option value="by_node">by_node</option>
							</select>
						</label>
					</div>
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
							disabled={cyclePolicy === "inherit_user"}
						/>
						{cyclePolicy === "inherit_user" ? (
							<p className="text-xs opacity-70">
								Cycle day is inherited from the user.
							</p>
						) : null}
					</label>
					<label className="form-control">
						<div className="label">
							<span className="label-text">Note</span>
							<span className="label-text-alt">Leave empty to clear</span>
						</div>
						<textarea
							className={textareaClass}
							value={note}
							onChange={(event) => setNote(event.target.value)}
						/>
					</label>
					{formError ? <p className="text-sm text-error">{formError}</p> : null}
					<div className="card-actions justify-end">
						<Button type="submit" loading={isSaving}>
							Save changes
						</Button>
					</div>
				</div>
			</form>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-3">
					<h2 className="card-title">Usage</h2>
					{usageQuery.isLoading ? (
						<p className="text-sm opacity-70">Loading usage...</p>
					) : usageNotImplemented ? (
						<p className="text-sm opacity-70">N/A</p>
					) : usageQuery.isError ? (
						<p className="text-sm text-error">
							{formatError(usageQuery.error)}
						</p>
					) : usageQuery.data ? (
						<div className="space-y-2">
							<div className="grid gap-2 md:grid-cols-2">
								<p>
									Cycle start:{" "}
									<span className="font-mono">
										{usageQuery.data.cycle_start_at}
									</span>
								</p>
								<p>
									Cycle end:{" "}
									<span className="font-mono">
										{usageQuery.data.cycle_end_at}
									</span>
								</p>
								<p>
									Used bytes:{" "}
									<span className="font-mono">
										{usageQuery.data.used_bytes}
									</span>
								</p>
								<p>
									Owner node:{" "}
									<span className="font-mono">
										{usageQuery.data.owner_node_id}
									</span>
								</p>
								<p>
									Desired enabled:{" "}
									{usageQuery.data.desired_enabled ? "Yes" : "No"}
								</p>
								<p>
									Effective enabled:{" "}
									{usageQuery.data.effective_enabled ? "Yes" : "No"}
								</p>
								<p>
									Quota banned: {usageQuery.data.quota_banned ? "Yes" : "No"}
								</p>
								<p>
									Banned at:{" "}
									<span className="font-mono">
										{usageQuery.data.quota_banned_at ?? "-"}
									</span>
								</p>
							</div>
							{usageQuery.data.warning ? (
								<p className="text-warning">
									Warning: {usageQuery.data.warning}
								</p>
							) : null}
						</div>
					) : (
						<p className="text-sm opacity-70">No usage data.</p>
					)}
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-3">
					<h2 className="card-title">Credentials</h2>
					{grant.credentials.vless ? (
						<div className="space-y-2">
							<p className="font-semibold">VLESS</p>
							<div className="flex flex-wrap items-center gap-2">
								<span className="text-sm opacity-70">UUID:</span>
								<span className="font-mono text-xs break-all">
									{grant.credentials.vless.uuid}
								</span>
								<CopyButton text={grant.credentials.vless.uuid} label="Copy" />
							</div>
							<div className="flex flex-wrap items-center gap-2">
								<span className="text-sm opacity-70">Email:</span>
								<span className="font-mono text-xs break-all">
									{grant.credentials.vless.email}
								</span>
								<CopyButton text={grant.credentials.vless.email} label="Copy" />
							</div>
						</div>
					) : null}
					{grant.credentials.ss2022 ? (
						<div className="space-y-2">
							<p className="font-semibold">SS2022</p>
							<div className="flex flex-wrap items-center gap-2">
								<span className="text-sm opacity-70">Method:</span>
								<span className="font-mono text-xs break-all">
									{grant.credentials.ss2022.method}
								</span>
							</div>
							<div className="flex flex-wrap items-center gap-2">
								<span className="text-sm opacity-70">Password:</span>
								<span className="font-mono text-xs break-all">
									{grant.credentials.ss2022.password}
								</span>
								<CopyButton
									text={grant.credentials.ss2022.password}
									label="Copy"
								/>
							</div>
						</div>
					) : null}
					{!grant.credentials.vless && !grant.credentials.ss2022 ? (
						<p className="text-sm opacity-70">No credentials available.</p>
					) : null}
				</div>
			</div>

			<div className="card bg-base-100 shadow border border-error/30">
				<div className="card-body space-y-3">
					<h2 className="card-title text-error">Danger zone</h2>
					<p className="text-sm opacity-70">
						Deleting a grant removes the quota allocation immediately.
					</p>
					<div className="card-actions justify-end">
						<Button variant="ghost" onClick={() => setDeleteOpen(true)}>
							Delete grant
						</Button>
					</div>
				</div>
			</div>

			<ConfirmDialog
				open={disableOpen}
				title="Disable grant"
				description="Disabling a grant revokes access immediately. Are you sure?"
				confirmLabel={isSaving ? "Disabling..." : "Disable"}
				onCancel={() => setDisableOpen(false)}
				onConfirm={() => {
					void handleSave(true);
				}}
			/>
			<ConfirmDialog
				open={deleteOpen}
				title="Delete grant"
				description="This action cannot be undone. Are you sure?"
				confirmLabel={isDeleting ? "Deleting..." : "Delete"}
				onCancel={() => setDeleteOpen(false)}
				onConfirm={async () => {
					setIsDeleting(true);
					try {
						await deleteAdminGrant(adminToken, grant.grant_id);
						pushToast({
							variant: "success",
							message: "Grant deleted.",
						});
						navigate({ to: "/grants" });
					} catch (err) {
						pushToast({
							variant: "error",
							message: `Failed to delete grant: ${formatError(err)}`,
						});
					} finally {
						setIsDeleting(false);
						setDeleteOpen(false);
					}
				}}
			/>
		</div>
	);
}
