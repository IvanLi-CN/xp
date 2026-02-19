import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";

import { runAdminEndpointProbeRun } from "../api/adminEndpointProbes";
import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { EndpointsTable } from "../components/EndpointsTable";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
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

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
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

		const nodes = nodesQuery.data?.items ?? [];
		const nodeById = new Map(nodes.map((n) => [n.node_id, n] as const));
		return <EndpointsTable endpoints={endpoints} nodeById={nodeById} />;
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
