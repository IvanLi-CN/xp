import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";

import { fetchAdminGrantGroups } from "../api/adminGrantGroups";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { ResourceTable } from "../components/ResourceTable";
import { readAdminToken } from "../components/auth";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

export function GrantGroupsPage() {
	const adminToken = readAdminToken();
	const groupsQuery = useQuery({
		queryKey: ["adminGrantGroups", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminGrantGroups(adminToken, signal),
	});

	const actions =
		adminToken.length === 0 ? (
			<Link to="/login" className="btn btn-primary">
				Go to login
			</Link>
		) : (
			<Link to="/grant-groups/new" className="btn btn-primary">
				New group
			</Link>
		);

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Set an admin token to load grant groups."
				/>
			);
		}

		if (groupsQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading grant groups"
					description="Fetching grant groups from the xp API."
				/>
			);
		}

		if (groupsQuery.isError) {
			return (
				<PageState
					variant="error"
					title="Failed to load grant groups"
					description={formatError(groupsQuery.error)}
					action={
						<Button variant="secondary" onClick={() => groupsQuery.refetch()}>
							Retry
						</Button>
					}
				/>
			);
		}

		const groups = groupsQuery.data?.items ?? [];

		if (groups.length === 0) {
			return (
				<PageState
					variant="empty"
					title="No grant groups yet"
					description="Create the first group to manage access."
					action={
						<Link to="/grant-groups/new" className="btn btn-primary">
							New group
						</Link>
					}
				/>
			);
		}

		return (
			<ResourceTable
				headers={[
					{ key: "group_name", label: "Group name" },
					{ key: "member_count", label: "Members" },
					{ key: "actions", label: "" },
				]}
			>
				{groups.map((g) => (
					<tr key={g.group_name}>
						<td className="font-mono text-xs">
							<Link
								to="/grant-groups/$groupName"
								params={{ groupName: g.group_name }}
								className="link link-primary"
							>
								{g.group_name}
							</Link>
						</td>
						<td className="font-mono text-xs">{g.member_count}</td>
						<td>
							<Link
								to="/grant-groups/$groupName"
								params={{ groupName: g.group_name }}
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
				title="Grant groups"
				description="Manage access in group-level transactions."
				actions={actions}
			/>
			{content}
		</div>
	);
}
