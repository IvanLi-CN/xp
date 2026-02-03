import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useMemo } from "react";

import { fetchAdminUserQuotaSummaries } from "../api/adminUserQuotaSummaries";
import { fetchAdminUsers } from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { ResourceTable } from "../components/ResourceTable";
import { readAdminToken } from "../components/auth";
import { formatQuotaBytesHuman } from "../utils/quota";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

export function UsersPage() {
	const adminToken = readAdminToken();
	const usersQuery = useQuery({
		queryKey: ["adminUsers", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminUsers(adminToken, signal),
	});
	const quotaSummariesQuery = useQuery({
		queryKey: ["adminUserQuotaSummaries", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminUserQuotaSummaries(adminToken, signal),
	});
	const quotaSummaryByUserId = useMemo(() => {
		const items = quotaSummariesQuery.data?.items ?? [];
		return new Map(items.map((s) => [s.user_id, s]));
	}, [quotaSummariesQuery.data]);

	const actions =
		adminToken.length === 0 ? (
			<Link to="/login" className="btn btn-primary">
				Go to login
			</Link>
		) : (
			<Link to="/users/new" className="btn btn-primary">
				New user
			</Link>
		);

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Set an admin token to load users."
				/>
			);
		}

		if (usersQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading users"
					description="Fetching admin users from the xp API."
				/>
			);
		}

		if (usersQuery.isError) {
			return (
				<PageState
					variant="error"
					title="Failed to load users"
					description={formatError(usersQuery.error)}
					action={
						<Button variant="secondary" onClick={() => usersQuery.refetch()}>
							Retry
						</Button>
					}
				/>
			);
		}

		const users = usersQuery.data?.items ?? [];

		if (users.length === 0) {
			return (
				<PageState
					variant="empty"
					title="No users yet"
					description="Create the first user to start managing subscriptions."
				/>
			);
		}

		return (
			<ResourceTable
				headers={[
					{ key: "user_id", label: "User ID" },
					{ key: "display_name", label: "Display name" },
					{
						key: "quota_summary",
						label: (
							<span
								className={
									quotaSummariesQuery.data?.partial ? "text-warning" : undefined
								}
								title={
									quotaSummariesQuery.isError
										? `Failed to load quota summaries: ${formatError(quotaSummariesQuery.error)}`
										: quotaSummariesQuery.data?.partial
											? `Partial data (unreachable nodes): ${quotaSummariesQuery.data.unreachable_nodes.join(", ")}`
											: undefined
								}
							>
								Quota usage (remaining/limit)
							</span>
						),
					},
					{ key: "quota_reset", label: "Quota reset" },
					{ key: "subscription_token", label: "Subscription token" },
					{ key: "actions", label: "" },
				]}
			>
				{users.map((user) => (
					<tr key={user.user_id}>
						<td className="font-mono text-xs">
							<Link
								to="/users/$userId"
								params={{ userId: user.user_id }}
								className="link link-primary"
							>
								{user.user_id}
							</Link>
						</td>
						<td>{user.display_name}</td>
						<td className="font-mono text-xs">
							{quotaSummariesQuery.isLoading ? (
								<span className="opacity-60">...</span>
							) : quotaSummariesQuery.isError ? (
								<span className="opacity-60">-</span>
							) : (
								(() => {
									const summary = quotaSummaryByUserId.get(user.user_id);
									if (!summary) return <span className="opacity-60">-</span>;
									const used = formatQuotaBytesHuman(summary.used_bytes);
									const remaining = formatQuotaBytesHuman(
										summary.remaining_bytes,
									);
									const limit = formatQuotaBytesHuman(
										summary.quota_limit_bytes,
									);
									return (
										<span title={`Used: ${used}`}>
											{remaining}/{limit}
										</span>
									);
								})()
							)}
						</td>
						<td className="font-mono text-xs">
							{user.quota_reset.policy === "monthly"
								? `monthly@${user.quota_reset.day_of_month} tz=${user.quota_reset.tz_offset_minutes}`
								: `unlimited tz=${user.quota_reset.tz_offset_minutes}`}
						</td>
						<td className="font-mono text-xs">{user.subscription_token}</td>
						<td>
							<Link
								to="/users/$userId"
								params={{ userId: user.user_id }}
								className="link"
							>
								Details
							</Link>
						</td>
					</tr>
				))}
			</ResourceTable>
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="Users"
				description="Manage subscription owners and defaults."
				actions={actions}
			/>
			{content}
		</div>
	);
}
