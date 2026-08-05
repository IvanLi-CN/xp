import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";

import { buttonVariants } from "@/components/ui/button";

import { runAdminEndpointProbeRun } from "../api/adminEndpointProbes";
import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { useApiCapability } from "../api/useApiCompatibility";
import { Button } from "../components/Button";
import { EndpointsTable } from "../components/EndpointsTable";
import { PageHeader } from "../components/PageHeader";
import { CapabilityUnavailableState, PageState } from "../components/PageState";
import { ReadStateBanner } from "../components/ReadStateBanner";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { useAppRuntime } from "../offline/appRuntime";
import {
	formatSyncTimestamp,
	hasQueryData,
	latestQueryDataUpdatedAt,
	queryIsOfflineBlocked,
} from "../offline/queryReadState";

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
	const runtime = useAppRuntime();
	const endpointsCapability = useApiCapability("admin.endpoints");
	const nodesCapability = useApiCapability("admin.nodes");
	const probesCapability = useApiCapability("admin.node-probes");
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const { pushToast } = useToast();
	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0 && endpointsCapability.available,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0 && nodesCapability.available,
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
			<Link className={buttonVariants()} to="/login">
				Go to login
			</Link>
		) : (
			<>
				<Button
					variant="secondary"
					loading={probeRunMutation.isPending}
					disabled={runtime.isReadOnly || !probesCapability.available}
					onClick={() => probeRunMutation.mutate()}
				>
					Test all now
				</Button>
				{runtime.isReadOnly ? (
					<Button disabled>New endpoint</Button>
				) : (
					<Link className={buttonVariants()} to="/endpoints/new">
						New endpoint
					</Link>
				)}
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

		if (endpointsCapability.unavailable) {
			return (
				<CapabilityUnavailableState
					title="Endpoints unavailable"
					reason={endpointsCapability.reason}
				/>
			);
		}
		if (nodesCapability.unavailable) {
			return (
				<CapabilityUnavailableState
					title="Endpoint node data unavailable"
					reason={nodesCapability.reason}
				/>
			);
		}

		if (endpointsQuery.isLoading && !hasQueryData(endpointsQuery)) {
			return (
				<PageState
					variant="loading"
					title="Loading endpoints"
					description="Fetching endpoints from the xp API."
				/>
			);
		}

		if (
			!hasQueryData(endpointsQuery) &&
			queryIsOfflineBlocked(endpointsQuery, runtime.isOnline)
		) {
			return (
				<PageState
					variant="offline"
					title="Offline cache unavailable"
					description="Open Endpoints once while online to keep the latest list available offline."
				/>
			);
		}

		if (endpointsQuery.isError && !hasQueryData(endpointsQuery)) {
			const description = formatErrorMessage(endpointsQuery.error);
			return (
				<PageState
					variant="error"
					title="Failed to load endpoints"
					description={description}
					error={endpointsQuery.error}
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
						<Link className={buttonVariants()} to="/endpoints/new">
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
	const latestSyncedAt = latestQueryDataUpdatedAt([endpointsQuery, nodesQuery]);
	const showCachedBanner =
		latestSyncedAt !== null &&
		hasQueryData(endpointsQuery) &&
		(!runtime.isOnline || endpointsQuery.isError || nodesQuery.isError);

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoints"
				description="Manage ingress endpoints for the cluster."
				actions={<div className="flex flex-wrap gap-2">{actions}</div>}
			/>
			{showCachedBanner ? (
				<ReadStateBanner
					tone={!runtime.isOnline ? "warning" : "info"}
					variant="inline"
					dismissible
					errors={[endpointsQuery.error, nodesQuery.error]}
					title={
						!runtime.isOnline
							? "Offline endpoint list"
							: "Showing cached endpoint data"
					}
					description={`Last successful sync: ${formatSyncTimestamp(latestSyncedAt)}.`}
				/>
			) : null}
			{content}
		</div>
	);
}
