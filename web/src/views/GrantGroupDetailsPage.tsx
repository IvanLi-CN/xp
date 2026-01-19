import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "@tanstack/react-router";

import { fetchAdminGrantGroup } from "../api/adminGrantGroups";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { readAdminToken } from "../components/auth";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

export function GrantGroupDetailsPage() {
	const adminToken = readAdminToken();
	const { groupName } = useParams({ from: "/app/grant-groups/$groupName" });

	const groupQuery = useQuery({
		queryKey: ["adminGrantGroup", adminToken, groupName],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminGrantGroup(adminToken, groupName, signal),
	});

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to load grant group details."
				action={
					<Link to="/login" className="btn btn-primary">
						Go to login
					</Link>
				}
			/>
		);
	}

	if (groupQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading grant group"
				description="Fetching grant group details from the control plane."
			/>
		);
	}

	if (groupQuery.isError) {
		return (
			<PageState
				variant="error"
				title="Failed to load grant group"
				description={formatError(groupQuery.error)}
				action={
					<Button variant="secondary" onClick={() => groupQuery.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	}

	if (!groupQuery.data) {
		return (
			<PageState
				variant="empty"
				title="Grant group not found"
				description="The group name does not exist."
				action={
					<Link
						to="/grants/new"
						className="btn btn-outline btn-sm xp-btn-outline"
					>
						Back to create
					</Link>
				}
			/>
		);
	}

	const detail = groupQuery.data;

	return (
		<div className="space-y-6">
			<PageHeader
				title="Grant group"
				description={
					<>
						Group: <span className="font-mono">{detail.group.group_name}</span>{" "}
						— {detail.members.length} member(s)
					</>
				}
				actions={
					<div className="flex items-center gap-2">
						<Link
							to="/grants/new"
							className="btn btn-outline btn-sm xp-btn-outline"
						>
							New group
						</Link>
						<Link to="/grants" className="btn btn-ghost btn-sm">
							Back
						</Link>
					</div>
				}
			/>

			<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-4">
				<h2 className="text-lg font-semibold">Members</h2>
				<div className="overflow-auto rounded-box border border-base-200">
					<table className="table">
						<thead>
							<tr>
								<th>User</th>
								<th>Endpoint</th>
								<th>Enabled</th>
								<th className="text-right">Quota (bytes)</th>
								<th>Note</th>
							</tr>
						</thead>
						<tbody>
							{detail.members.map((m) => (
								<tr key={`${m.user_id}:${m.endpoint_id}`}>
									<td className="font-mono text-xs">{m.user_id}</td>
									<td className="font-mono text-xs">{m.endpoint_id}</td>
									<td>{m.enabled ? "yes" : "no"}</td>
									<td className="text-right font-mono text-xs">
										{m.quota_limit_bytes}
									</td>
									<td className="text-sm">{m.note ?? "-"}</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>
			</div>
		</div>
	);
}
