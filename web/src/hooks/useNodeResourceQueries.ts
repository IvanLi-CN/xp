import { useQueries, useQuery } from "@tanstack/react-query";

import {
	NODE_RESOURCE_HISTORY_METRICS,
	type NodeResourceHistoryMetric,
	RUNTIME_RESOURCE_HISTORY_METRICS,
	type ResourceRole,
	type RuntimeResourceHistoryMetric,
	fetchAdminNodeResourceHistory,
	fetchAdminNodeResources,
} from "../api/adminResources";
import { useApiCapability } from "../api/useApiCompatibility";

export function resourceQueryRefetchInterval(
	isOnline: boolean,
	status: string,
	intervalMs: number,
): number | false {
	return isOnline && status !== "error" ? intervalMs : false;
}

export function useNodeResourceQueries(props: {
	adminToken: string;
	nodeId: string;
	nodesAvailable: boolean;
	isOnline: boolean;
	activeTab: string;
	selectedRuntimeRole: ResourceRole | null;
}) {
	const resourceCapability = useApiCapability("admin.resource-monitoring");
	const enabled =
		props.adminToken.length > 0 &&
		props.nodesAvailable &&
		resourceCapability.available;
	const resourceQuery = useQuery({
		queryKey: ["adminNodeResources", props.adminToken, props.nodeId],
		enabled,
		queryFn: ({ signal }) =>
			fetchAdminNodeResources(props.adminToken, props.nodeId, signal),
		refetchInterval: (query: { state: { status: string } }) =>
			resourceQueryRefetchInterval(props.isOnline, query.state.status, 15_000),
	});
	const resourceHistoryQueries = useQueries({
		queries: NODE_RESOURCE_HISTORY_METRICS.map((metric) => ({
			queryKey: [
				"adminNodeResourceHistory",
				props.adminToken,
				props.nodeId,
				metric,
			],
			enabled: enabled && props.activeTab === "resources",
			queryFn: ({ signal }: { signal: AbortSignal }) =>
				fetchAdminNodeResourceHistory(
					props.adminToken,
					props.nodeId,
					metric,
					signal,
				),
			refetchInterval: (query: { state: { status: string } }) =>
				resourceQueryRefetchInterval(
					props.isOnline,
					query.state.status,
					30_000,
				),
		})),
	});
	const resourceHistoryByMetric = Object.fromEntries(
		NODE_RESOURCE_HISTORY_METRICS.map((metric, index) => [
			metric,
			resourceHistoryQueries[index]?.data?.points ?? [],
		]),
	);
	const resourceHistoryErrorByMetric = Object.fromEntries(
		NODE_RESOURCE_HISTORY_METRICS.map((metric, index) => [
			metric,
			resourceHistoryQueries[index]?.error,
		]),
	) as Partial<Record<NodeResourceHistoryMetric, unknown>>;
	const runtimeHistoryQueries = useQueries({
		queries: RUNTIME_RESOURCE_HISTORY_METRICS.map((metric) => ({
			queryKey: [
				"adminNodeRuntimeResourceHistory",
				props.adminToken,
				props.nodeId,
				props.selectedRuntimeRole,
				metric,
			],
			enabled:
				enabled &&
				props.activeTab === "resources" &&
				props.selectedRuntimeRole !== null,
			queryFn: ({ signal }: { signal: AbortSignal }) =>
				fetchAdminNodeResourceHistory(
					props.adminToken,
					props.nodeId,
					metric,
					signal,
					props.selectedRuntimeRole ?? undefined,
				),
			refetchInterval: (query: { state: { status: string } }) =>
				resourceQueryRefetchInterval(
					props.isOnline,
					query.state.status,
					30_000,
				),
		})),
	});
	const runtimeHistoryByMetric = Object.fromEntries(
		RUNTIME_RESOURCE_HISTORY_METRICS.map((metric, index) => [
			metric,
			runtimeHistoryQueries[index]?.data?.points ?? [],
		]),
	);
	const runtimeHistoryErrorByMetric = Object.fromEntries(
		RUNTIME_RESOURCE_HISTORY_METRICS.map((metric, index) => [
			metric,
			runtimeHistoryQueries[index]?.error,
		]),
	) as Partial<Record<RuntimeResourceHistoryMetric, unknown>>;
	const retryResourceHistory = (metric: NodeResourceHistoryMetric) => {
		const index = NODE_RESOURCE_HISTORY_METRICS.indexOf(metric);
		void resourceHistoryQueries[index]?.refetch();
	};
	const retryRuntimeHistory = (metric: RuntimeResourceHistoryMetric) => {
		const index = RUNTIME_RESOURCE_HISTORY_METRICS.indexOf(metric);
		void runtimeHistoryQueries[index]?.refetch();
	};
	return {
		resourceCapability,
		resourceQuery,
		resourceHistoryByMetric,
		resourceHistoryErrorByMetric,
		retryResourceHistory,
		runtimeHistoryByMetric,
		runtimeHistoryErrorByMetric,
		retryRuntimeHistory,
	};
}
