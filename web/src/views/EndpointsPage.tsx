import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageState } from "../components/PageState";
import { ResourceTable } from "../components/ResourceTable";
import { readAdminToken } from "../components/auth";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	if (error instanceof Error) return error.message;
	return String(error);
}

export function EndpointsPage() {
	const adminToken = readAdminToken();
	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to load endpoints."
				action={
					<Link className="btn btn-primary" to="/login">
						Go to login
					</Link>
				}
			/>
		);
	}

	if (endpointsQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading endpoints"
				description="Fetching endpoints from the control plane."
			/>
		);
	}

	if (endpointsQuery.isError) {
		const description = formatErrorMessage(endpointsQuery.error);
		return (
			<PageState
				variant="error"
				title="Failed to load endpoints"
				description={description}
				action={
					<Button variant="secondary" onClick={() => endpointsQuery.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	}

	const endpoints = endpointsQuery.data?.items ?? [];

	if (endpoints.length === 0) {
		return (
			<PageState
				variant="empty"
				title="No endpoints yet"
				description="Create your first endpoint to start serving traffic."
				action={
					<Link className="btn btn-primary" to="/endpoints/new">
						Create endpoint
					</Link>
				}
			/>
		);
	}

	return (
		<div className="space-y-6">
			<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div>
					<h1 className="text-2xl font-bold">Endpoints</h1>
					<p className="text-sm opacity-70">
						Manage ingress endpoints for the cluster.
					</p>
				</div>
				<div className="flex gap-2">
					<Link className="btn btn-primary" to="/endpoints/new">
						New endpoint
					</Link>
					<Button
						variant="secondary"
						loading={endpointsQuery.isFetching}
						onClick={() => endpointsQuery.refetch()}
					>
						Refresh
					</Button>
				</div>
			</div>

			<ResourceTable
				headers={[
					{ key: "kind", label: "Kind" },
					{ key: "node", label: "Node" },
					{ key: "port", label: "Port" },
					{ key: "tag", label: "Tag" },
					{ key: "endpoint", label: "Endpoint ID" },
				]}
			>
				{endpoints.map((endpoint) => (
					<tr key={endpoint.endpoint_id}>
						<td className="font-mono text-xs">{endpoint.kind}</td>
						<td className="font-mono text-xs">{endpoint.node_id}</td>
						<td className="font-mono text-xs">{endpoint.port}</td>
						<td>
							<Link
								className="link link-primary font-mono text-xs"
								to="/endpoints/$endpointId"
								params={{ endpointId: endpoint.endpoint_id }}
							>
								{endpoint.tag}
							</Link>
						</td>
						<td>
							<Link
								className="link link-secondary font-mono text-xs"
								to="/endpoints/$endpointId"
								params={{ endpointId: endpoint.endpoint_id }}
							>
								{endpoint.endpoint_id}
							</Link>
						</td>
					</tr>
				))}
			</ResourceTable>
		</div>
	);
}
