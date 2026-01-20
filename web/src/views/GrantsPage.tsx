import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";

import { fetchAdminGrants } from "../api/adminGrants";
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

export function GrantsPage() {
	const adminToken = readAdminToken();
	const grantsQuery = useQuery({
		queryKey: ["adminGrants", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminGrants(adminToken, signal),
	});

	const actions =
		adminToken.length === 0 ? (
			<Link to="/login" className="btn btn-primary">
				Go to login
			</Link>
		) : (
			<Link to="/grants/new" className="btn btn-primary">
				New grant
			</Link>
		);

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Set an admin token to load grants."
				/>
			);
		}

		if (grantsQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading grants"
					description="Fetching grant list from the xp API."
				/>
			);
		}

		if (grantsQuery.isError) {
			return (
				<PageState
					variant="error"
					title="Failed to load grants"
					description={formatError(grantsQuery.error)}
					action={
						<Button variant="secondary" onClick={() => grantsQuery.refetch()}>
							Retry
						</Button>
					}
				/>
			);
		}

		const grants = grantsQuery.data?.items ?? [];

		if (grants.length === 0) {
			return (
				<PageState
					variant="empty"
					title="No grants yet"
					description="Create the first grant to allocate quota."
					action={
						<Link to="/grants/new" className="btn btn-primary">
							New grant
						</Link>
					}
				/>
			);
		}

		return (
			<ResourceTable
				headers={[
					{ key: "grant_id", label: "Grant ID" },
					{ key: "user_id", label: "User" },
					{ key: "endpoint_id", label: "Endpoint" },
					{ key: "enabled", label: "Enabled" },
					{ key: "quota_limit_bytes", label: "Quota (bytes)" },
					{ key: "cycle_policy", label: "Cycle" },
					{ key: "note", label: "Note" },
					{ key: "actions", label: "" },
				]}
			>
				{grants.map((grant) => (
					<tr key={grant.grant_id}>
						<td className="font-mono text-xs">
							<Link
								to="/grants/$grantId"
								params={{ grantId: grant.grant_id }}
								className="link link-primary"
							>
								{grant.grant_id}
							</Link>
						</td>
						<td className="font-mono text-xs">{grant.user_id}</td>
						<td className="font-mono text-xs">{grant.endpoint_id}</td>
						<td>{grant.enabled ? "Yes" : "No"}</td>
						<td className="font-mono text-xs">{grant.quota_limit_bytes}</td>
						<td className="font-mono text-xs">{grant.cycle_policy}</td>
						<td>{grant.note ?? "-"}</td>
						<td>
							<Link
								to="/grants/$grantId"
								params={{ grantId: grant.grant_id }}
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
				title="Grants"
				description="Quota allocations per user and endpoint."
				actions={actions}
			/>
			{content}
		</div>
	);
}
