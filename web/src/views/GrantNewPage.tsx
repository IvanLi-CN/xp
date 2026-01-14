import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { type CyclePolicy, createAdminGrant } from "../api/adminGrants";
import { fetchAdminUsers } from "../api/adminUsers";
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

export function GrantNewPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
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
			? "textarea textarea-bordered textarea-sm"
			: "textarea textarea-bordered";

	const usersQuery = useQuery({
		queryKey: ["adminUsers", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminUsers(adminToken, signal),
	});

	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});

	const [userId, setUserId] = useState("");
	const [endpointId, setEndpointId] = useState("");
	const [quotaLimit, setQuotaLimit] = useState(0);
	const [cyclePolicy, setCyclePolicy] = useState<CyclePolicy>("inherit_user");
	const [cycleDay, setCycleDay] = useState(1);
	const [note, setNote] = useState("");
	const [error, setError] = useState<string | null>(null);
	const [isSubmitting, setIsSubmitting] = useState(false);

	useEffect(() => {
		if (!usersQuery.data) return;
		if (!userId && usersQuery.data.items.length > 0) {
			setUserId(usersQuery.data.items[0].user_id);
		}
	}, [userId, usersQuery.data]);

	useEffect(() => {
		if (!endpointsQuery.data) return;
		if (!endpointId && endpointsQuery.data.items.length > 0) {
			setEndpointId(endpointsQuery.data.items[0].endpoint_id);
		}
	}, [endpointId, endpointsQuery.data]);

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Set an admin token to create grants."
					action={
						<Link to="/login" className="btn btn-primary">
							Go to login
						</Link>
					}
				/>
			);
		}

		if (usersQuery.isLoading || endpointsQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading grant form"
					description="Fetching users and endpoints."
				/>
			);
		}

		if (usersQuery.isError || endpointsQuery.isError) {
			const message = usersQuery.isError
				? formatError(usersQuery.error)
				: endpointsQuery.isError
					? formatError(endpointsQuery.error)
					: "Unknown error";
			return (
				<PageState
					variant="error"
					title="Failed to load grant form"
					description={message}
					action={
						<Button
							variant="secondary"
							onClick={() => {
								usersQuery.refetch();
								endpointsQuery.refetch();
							}}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		const users = usersQuery.data?.items ?? [];
		const endpoints = endpointsQuery.data?.items ?? [];

		if (users.length === 0 || endpoints.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Missing dependencies"
					description={
						users.length === 0
							? "Create a user before creating grants."
							: "Create an endpoint before creating grants."
					}
				/>
			);
		}

		return (
			<form
				className="card bg-base-100 shadow"
				onSubmit={async (event) => {
					event.preventDefault();
					if (!userId || !endpointId) {
						setError("User and endpoint are required.");
						return;
					}
					if (quotaLimit < 0) {
						setError("Quota limit must be zero or greater.");
						return;
					}
					if (cyclePolicy !== "inherit_user") {
						if (cycleDay < 1 || cycleDay > 31) {
							setError("Cycle day must be between 1 and 31.");
							return;
						}
					}
					setError(null);
					setIsSubmitting(true);
					try {
						const payload = {
							user_id: userId,
							endpoint_id: endpointId,
							quota_limit_bytes: quotaLimit,
							cycle_policy: cyclePolicy,
							cycle_day_of_month:
								cyclePolicy === "inherit_user" ? null : cycleDay,
							note: note.trim() ? note.trim() : null,
						};
						const created = await createAdminGrant(adminToken, payload);
						pushToast({
							variant: "success",
							message: "Grant created.",
						});
						navigate({
							to: "/grants/$grantId",
							params: { grantId: created.grant_id },
						});
					} catch (err) {
						setError(formatError(err));
						pushToast({
							variant: "error",
							message: "Failed to create grant.",
						});
					} finally {
						setIsSubmitting(false);
					}
				}}
			>
				<div className="card-body space-y-4">
					<div className="grid gap-4 md:grid-cols-2">
						<label className="form-control">
							<div className="label">
								<span className="label-text">User</span>
							</div>
							<select
								className={selectClass}
								value={userId}
								onChange={(event) => setUserId(event.target.value)}
							>
								{users.map((user) => (
									<option key={user.user_id} value={user.user_id}>
										{user.display_name} ({user.user_id})
									</option>
								))}
							</select>
						</label>
						<label className="form-control">
							<div className="label">
								<span className="label-text">Endpoint</span>
							</div>
							<select
								className={selectClass}
								value={endpointId}
								onChange={(event) => setEndpointId(event.target.value)}
							>
								{endpoints.map((endpoint) => (
									<option
										key={endpoint.endpoint_id}
										value={endpoint.endpoint_id}
									>
										{endpoint.tag} ({endpoint.endpoint_id})
									</option>
								))}
							</select>
						</label>
					</div>
					<div className="grid gap-4 md:grid-cols-2">
						<label className="form-control">
							<div className="label">
								<span className="label-text">Quota limit (bytes)</span>
							</div>
							<input
								className={inputClass}
								type="number"
								min={0}
								value={quotaLimit}
								onChange={(event) => setQuotaLimit(Number(event.target.value))}
							/>
						</label>
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
							<span className="label-text">Note (optional)</span>
						</div>
						<textarea
							className={textareaClass}
							value={note}
							onChange={(event) => setNote(event.target.value)}
							placeholder="e.g. enterprise quota"
						/>
					</label>
					{error ? <p className="text-sm text-error">{error}</p> : null}
					<div className="card-actions justify-end">
						<Button type="submit" loading={isSubmitting}>
							Create grant
						</Button>
					</div>
				</div>
			</form>
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="New grant"
				description="Allocate quota to a user on an endpoint."
				actions={
					<Link to="/grants" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>
			{content}
		</div>
	);
}
