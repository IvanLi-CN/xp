import { Link, useNavigate } from "@tanstack/react-router";
import { useState } from "react";

import { type CyclePolicyDefault, createAdminUser } from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
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

export function UserNewPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
	const { pushToast } = useToast();
	const prefs = useUiPrefs();
	const [displayName, setDisplayName] = useState("");
	const [cyclePolicy, setCyclePolicy] = useState<CyclePolicyDefault>("by_user");
	const [cycleDay, setCycleDay] = useState(1);
	const [error, setError] = useState<string | null>(null);
	const [isSubmitting, setIsSubmitting] = useState(false);

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";
	const selectClass =
		prefs.density === "compact"
			? "select select-bordered select-sm"
			: "select select-bordered";

	const content =
		adminToken.length === 0 ? (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to create users."
				action={
					<Link to="/login" className="btn btn-primary">
						Go to login
					</Link>
				}
			/>
		) : (
			<form
				className="card bg-base-100 shadow"
				onSubmit={async (event) => {
					event.preventDefault();
					const trimmedName = displayName.trim();
					if (!trimmedName) {
						setError("Display name is required.");
						return;
					}
					if (cycleDay < 1 || cycleDay > 31) {
						setError("Cycle day must be between 1 and 31.");
						return;
					}
					setError(null);
					setIsSubmitting(true);
					try {
						const created = await createAdminUser(adminToken, {
							display_name: trimmedName,
							cycle_policy_default: cyclePolicy,
							cycle_day_of_month_default: cycleDay,
						});
						pushToast({
							variant: "success",
							message: "User created.",
						});
						navigate({
							to: "/users/$userId",
							params: { userId: created.user_id },
						});
					} catch (err) {
						setError(formatError(err));
						pushToast({
							variant: "error",
							message: "Failed to create user.",
						});
					} finally {
						setIsSubmitting(false);
					}
				}}
			>
				<div className="card-body space-y-4">
					<label className="form-control">
						<div className="label">
							<span className="label-text">Display name</span>
						</div>
						<input
							className={inputClass}
							value={displayName}
							onChange={(event) => setDisplayName(event.target.value)}
							placeholder="e.g. Customer A"
							required
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
								required
							/>
						</label>
					</div>
					{error ? <p className="text-sm text-error">{error}</p> : null}
					<div className="card-actions justify-end">
						<Button type="submit" loading={isSubmitting}>
							Create user
						</Button>
					</div>
				</div>
			</form>
		);

	return (
		<div className="space-y-6">
			<PageHeader
				title="New user"
				description="Create a new subscription owner."
				actions={
					<Link to="/users" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>
			{content}
		</div>
	);
}
