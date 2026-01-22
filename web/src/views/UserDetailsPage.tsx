import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState } from "react";

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

export function UserDetailsPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
	const { userId } = useParams({ from: "/app/users/$userId" });
	const { pushToast } = useToast();
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

	const nodeQuotasQuery = useQuery({
		queryKey: ["adminUserNodeQuotas", adminToken, userId],
		enabled: adminToken.length > 0 && userId.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminUserNodeQuotas(adminToken, userId, signal),
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
	const [subLoading, setSubLoading] = useState(false);
	const [subText, setSubText] = useState("");

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
		try {
			const text = await fetchSubscription(subscriptionToken, subFormat);
			setSubText(text);
		} catch (err) {
			setSubText(formatError(err));
		} finally {
			setSubLoading(false);
		}
	}, [subFormat, subscriptionToken]);

	useEffect(() => {
		void loadSubscriptionPreview();
	}, [loadSubscriptionPreview]);

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
						<Link to="/grant-groups/new" className="btn btn-outline btn-sm">
							New grant group
						</Link>
						<Link to="/users" className="btn btn-ghost btn-sm">
							Back
						</Link>
					</div>
				}
			/>

			<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
				<div className="flex flex-wrap items-center justify-between gap-3">
					<div className="space-y-1">
						<div className="text-xs opacity-70">Subscription token</div>
						<div className="flex items-center gap-2">
							<span className="font-mono text-xs">
								{user.subscription_token}
							</span>
							<CopyButton text={user.subscription_token} />
						</div>
					</div>
					<div className="flex items-center gap-2">
						<Button
							variant="secondary"
							onClick={() => setResetTokenOpen(true)}
							disabled={isResettingToken}
						>
							Reset token
						</Button>
						<Button
							variant="danger"
							onClick={() => setDeleteOpen(true)}
							disabled={isDeleting}
						>
							Delete user
						</Button>
					</div>
				</div>

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
							onChange={(e) => setResetTzOffsetMinutes(Number(e.target.value))}
						/>
					</label>
				</div>

				{saveError ? <p className="text-sm text-error">{saveError}</p> : null}

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
						Save
					</Button>
				</div>
			</div>

			<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
				<div className="flex items-center justify-between gap-3">
					<h2 className="text-lg font-semibold">Node quotas</h2>
					<Button
						variant="secondary"
						onClick={() => nodeQuotasQuery.refetch()}
						disabled={nodeQuotasQuery.isFetching}
					>
						Refresh
					</Button>
				</div>
				<div className="space-y-3">
					{nodes.map((n) => (
						<div
							key={n.node_id}
							className="flex flex-wrap items-center justify-between gap-3 rounded-box border border-base-200 p-4"
						>
							<div className="space-y-1">
								<div className="font-medium">{n.node_name}</div>
								<div className="font-mono text-xs opacity-70">{n.node_id}</div>
							</div>
							<NodeQuotaEditor
								value={quotaByNodeId.get(n.node_id)?.quota_limit_bytes ?? 0}
								onApply={async (nextBytes) => {
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
								}}
							/>
						</div>
					))}
				</div>
			</div>

			<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
				<div className="flex items-center justify-between gap-3">
					<h2 className="text-lg font-semibold">Subscription preview</h2>
					<div className="flex items-center gap-2">
						<select
							className={selectClass}
							value={subFormat}
							onChange={(e) =>
								setSubFormat(e.target.value as SubscriptionFormat)
							}
						>
							<option value="raw">raw</option>
							<option value="clash">clash</option>
						</select>
						<Button
							variant="secondary"
							onClick={loadSubscriptionPreview}
							loading={subLoading}
						>
							Reload
						</Button>
					</div>
				</div>
				<textarea
					className={textareaClass}
					value={subText}
					readOnly
					placeholder="Subscription content…"
				/>
			</div>

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
