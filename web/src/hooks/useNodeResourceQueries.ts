import { useQueries, useQuery } from "@tanstack/react-query";

import {
	NODE_RESOURCE_HISTORY_METRICS,
	RUNTIME_RESOURCE_HISTORY_METRICS,
	type ResourceRole,
	fetchAdminNodeResourceHistory,
	fetchAdminNodeResources,
} from "../api/adminResources";
import { useApiCapability } from "../api/useApiCompatibility";

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
		refetchInterval: props.isOnline ? 15_000 : false,
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
			refetchInterval: props.isOnline ? 30_000 : false,
		})),
	});
	const resourceHistoryByMetric = Object.fromEntries(
		NODE_RESOURCE_HISTORY_METRICS.map((metric, index) => [
			metric,
			resourceHistoryQueries[index]?.data?.points ?? [],
		]),
	);
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
			refetchInterval: props.isOnline ? 30_000 : false,
		})),
	});
	const runtimeHistoryByMetric = Object.fromEntries(
		RUNTIME_RESOURCE_HISTORY_METRICS.map((metric, index) => [
			metric,
			runtimeHistoryQueries[index]?.data?.points ?? [],
		]),
	);
	return {
		resourceCapability,
		resourceQuery,
		resourceHistoryByMetric,
		runtimeHistoryByMetric,
	};
}
