import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";

import { runAdminEndpointProbeRun } from "../api/adminEndpointProbes";
import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { EndpointProbeBar } from "../components/EndpointProbeBar";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { ResourceTable } from "../components/ResourceTable";
import { useToast } from "../components/Toast";
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
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const { pushToast } = useToast();
	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});

	const probeRunMutation = useMutation({
		mutationFn: () => runAdminEndpointProbeRun(adminToken),
		onSuccess: (data) => {
			pushToast({
				variant: "success",
				message: `Probe started (hour=${data.hour}).`,
			});
			queryClient.invalidateQueries({
				queryKey: ["adminEndpoints", adminToken],
			});
			navigate({
				to: "/endpoints/probe/runs/$runId",
				params: { runId: data.run_id },
			});
		},
		onError: (error) => {
			pushToast({
				variant: "error",
				message: formatErrorMessage(error),
			});
		},
	});

	const actions =
		adminToken.length === 0 ? (
			<Link className="btn btn-primary" to="/login">
				Go to login
			</Link>
		) : (
			<>
				<Button
					variant="secondary"
					loading={probeRunMutation.isPending}
					onClick={() => probeRunMutation.mutate()}
				>
					Test all now
				</Button>
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
			</>
		);

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Set an admin token to load endpoints."
				/>
			);
		}

		if (endpointsQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading endpoints"
					description="Fetching endpoints from the xp API."
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
						<Button
							variant="secondary"
							onClick={() => endpointsQuery.refetch()}
						>
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
			<ResourceTable
				headers={[
					{ key: "probe", label: "Probe (24h)" },
					{ key: "latency", label: "Latency (p50 ms)" },
					{ key: "kind", label: "Kind" },
					{ key: "node", label: "Node" },
					{ key: "port", label: "Listen port" },
					{ key: "tag", label: "Tag" },
					{ key: "endpoint", label: "Endpoint ID" },
				]}
			>
				{endpoints.map((endpoint) => (
					<tr key={endpoint.endpoint_id}>
						<td>
							<Link
								className="inline-flex items-center"
								to="/endpoints/$endpointId/probe"
								params={{ endpointId: endpoint.endpoint_id }}
							>
								<EndpointProbeBar slots={endpoint.probe?.slots ?? []} />
							</Link>
						</td>
						<td className="font-mono text-xs">
							{endpoint.probe?.latest_latency_ms_p50 ?? "-"}
						</td>
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
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoints"
				description="Manage ingress endpoints for the cluster."
				actions={<div className="flex flex-wrap gap-2">{actions}</div>}
			/>
			{content}
		</div>
	);
}
