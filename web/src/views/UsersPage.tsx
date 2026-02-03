import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useMemo } from "react";

import { fetchAdminUserQuotaSummaries } from "../api/adminUserQuotaSummaries";
import { fetchAdminUsers } from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
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

function formatUtcOffset(minutes: number): string {
	const sign = minutes >= 0 ? "+" : "-";
	const abs = Math.abs(minutes);
	const hh = String(Math.floor(abs / 60)).padStart(2, "0");
	const mm = String(abs % 60).padStart(2, "0");
	return `UTC${sign}${hh}:${mm}`;
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
				tableClassName="table-fixed w-full"
				headers={[
					{ key: "user", label: "User" },
					{
						key: "access",
						label: (
							<div className="flex flex-col leading-tight">
								<span>Access</span>
								<span className="text-xs opacity-60 font-normal">
									token/reset
								</span>
							</div>
						),
					},
					{
						key: "quota_summary",
						label: (
							<div
								className={[
									"flex flex-col leading-tight",
									quotaSummariesQuery.data?.partial ? "text-warning" : "",
								]
									.filter(Boolean)
									.join(" ")}
								title={
									quotaSummariesQuery.isError
										? `Failed to load quota summaries: ${formatError(quotaSummariesQuery.error)}`
										: quotaSummariesQuery.data?.partial
											? `Partial data (unreachable nodes): ${quotaSummariesQuery.data.unreachable_nodes.join(", ")}`
											: "Remaining / limit (aggregated across nodes)"
								}
							>
								<span>Quota usage</span>
								<span className="text-xs opacity-60 font-normal">
									remaining/limit
								</span>
							</div>
						),
					},
				]}
			>
				{users.map((user) => (
					<tr key={user.user_id}>
						<td className="align-top">
							<div className="flex flex-col gap-1 min-w-0">
								<Link
									to="/users/$userId"
									params={{ userId: user.user_id }}
									className="link link-hover font-semibold block truncate"
									title="Open user details"
								>
									{user.display_name}
								</Link>
								<div className="flex items-center gap-2 min-w-0">
									<Link
										to="/users/$userId"
										params={{ userId: user.user_id }}
										className="link link-primary font-mono text-xs block truncate min-w-0"
										title={user.user_id}
									>
										{user.user_id}
									</Link>
									<CopyButton
										text={user.user_id}
										iconOnly
										variant="ghost"
										size="sm"
										ariaLabel="Copy user ID"
										className="px-2 shrink-0"
									/>
								</div>
							</div>
						</td>
						<td className="align-top">
							<div className="flex flex-col gap-1 min-w-0">
								<div className="flex items-center gap-2 min-w-0">
									<span
										className="font-mono text-xs block truncate min-w-0"
										title={user.subscription_token}
									>
										{user.subscription_token}
									</span>
									<CopyButton
										text={user.subscription_token}
										iconOnly
										variant="ghost"
										size="sm"
										ariaLabel="Copy subscription token"
										className="px-2 shrink-0"
									/>
								</div>
								<div
									className="text-xs opacity-60 whitespace-nowrap truncate"
									title="Quota reset policy (user default)"
								>
									{user.quota_reset.policy === "monthly"
										? `Reset: monthly@${user.quota_reset.day_of_month} ${formatUtcOffset(user.quota_reset.tz_offset_minutes)}`
										: `Reset: unlimited ${formatUtcOffset(user.quota_reset.tz_offset_minutes)}`}
								</div>
							</div>
						</td>
						<td className="align-top">
							<div className="flex flex-col gap-1 min-w-0">
								<div className="font-mono text-xs whitespace-nowrap">
									{quotaSummariesQuery.isLoading ? (
										<span className="opacity-60">...</span>
									) : quotaSummariesQuery.isError ? (
										<span className="opacity-60">-</span>
									) : (
										(() => {
											const summary = quotaSummaryByUserId.get(user.user_id);
											if (!summary)
												return <span className="opacity-60">-</span>;
											const remaining = formatQuotaBytesHuman(
												summary.remaining_bytes,
											);
											const limit = formatQuotaBytesHuman(
												summary.quota_limit_bytes,
											);
											return (
												<span title="Remaining / limit">
													{remaining}/{limit}
												</span>
											);
										})()
									)}
								</div>
								<div className="text-xs opacity-60 whitespace-nowrap truncate">
									{quotaSummariesQuery.isLoading || quotaSummariesQuery.isError
										? null
										: (() => {
												const summary = quotaSummaryByUserId.get(user.user_id);
												if (!summary) return null;
												const used = formatQuotaBytesHuman(summary.used_bytes);
												return quotaSummariesQuery.data?.partial
													? `Used: ${used} · partial`
													: `Used: ${used}`;
											})()}
								</div>
							</div>
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
