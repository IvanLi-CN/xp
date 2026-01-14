import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

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

	useEffect(() => {
		if (!userQuery.data) return;
		setDisplayName(userQuery.data.display_name);
		setCyclePolicy(userQuery.data.cycle_policy_default);
		setCycleDay(userQuery.data.cycle_day_of_month_default);
	}, [userQuery.data]);

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
					<Link to="/users" className="btn btn-secondary">
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
