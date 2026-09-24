import { useQueries, useQuery } from "@tanstack/react-query";
import { useState } from "react";

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

let resourcePageSessionSequence = 0;

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
	const [resourcePageSessionId] = useState(() => ++resourcePageSessionSequence);
	const enabled =
		props.adminToken.length > 0 &&
		props.nodesAvailable &&
		resourceCapability.available;
	const resourceQuery = useQuery({
		queryKey: [
			"adminNodeResources",
			resourcePageSessionId,
			props.adminToken,
			props.nodeId,
		],
		enabled,
		refetchOnMount: false,
		refetchOnReconnect: false,
		refetchOnWindowFocus: false,
		queryFn: ({ signal }) =>
			fetchAdminNodeResources(props.adminToken, props.nodeId, signal),
		retry: false,
		refetchInterval: (query: { state: { status: string } }) =>
			resourceQueryRefetchInterval(props.isOnline, query.state.status, 15_000),
	});
	const resourceHistoryQueries = useQueries({
		queries: NODE_RESOURCE_HISTORY_METRICS.map((metric) => ({
			queryKey: [
				"adminNodeResourceHistory",
				resourcePageSessionId,
				props.adminToken,
				props.nodeId,
				metric,
			],
			enabled: enabled && props.activeTab === "resources",
			refetchOnMount: false,
			refetchOnReconnect: false,
			refetchOnWindowFocus: false,
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
			retry: false,
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
	const resourceHistoryFetchingByMetric = Object.fromEntries(
		NODE_RESOURCE_HISTORY_METRICS.map((metric, index) => [
			metric,
			resourceHistoryQueries[index]?.isFetching ?? false,
		]),
	) as Partial<Record<NodeResourceHistoryMetric, boolean>>;
	const runtimeHistoryQueries = useQueries({
		queries: RUNTIME_RESOURCE_HISTORY_METRICS.map((metric) => ({
			queryKey: [
				"adminNodeRuntimeResourceHistory",
				resourcePageSessionId,
				props.adminToken,
				props.nodeId,
				props.selectedRuntimeRole,
				metric,
			],
			enabled:
				enabled &&
				props.activeTab === "resources" &&
				props.selectedRuntimeRole !== null,
			refetchOnMount: false,
			refetchOnReconnect: false,
			refetchOnWindowFocus: false,
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
			retry: false,
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
	const runtimeHistoryFetchingByMetric = Object.fromEntries(
		RUNTIME_RESOURCE_HISTORY_METRICS.map((metric, index) => [
			metric,
			runtimeHistoryQueries[index]?.isFetching ?? false,
		]),
	) as Partial<Record<RuntimeResourceHistoryMetric, boolean>>;
	const retryResourceHistory = (metric: NodeResourceHistoryMetric) => {
		const index = NODE_RESOURCE_HISTORY_METRICS.indexOf(metric);
		if (resourceHistoryQueries[index]?.isFetching) return;
		void resourceHistoryQueries[index]?.refetch();
	};
	const retryRuntimeHistory = (metric: RuntimeResourceHistoryMetric) => {
		const index = RUNTIME_RESOURCE_HISTORY_METRICS.indexOf(metric);
		if (runtimeHistoryQueries[index]?.isFetching) return;
		void runtimeHistoryQueries[index]?.refetch();
	};
	const resourceTabProps = {
		historyByMetric: resourceHistoryByMetric,
		historyErrorByMetric: resourceHistoryErrorByMetric,
		historyFetchingByMetric: resourceHistoryFetchingByMetric,
		runtimeHistoryByMetric,
		runtimeHistoryErrorByMetric,
		runtimeHistoryFetchingByMetric,
		onRetryHistory: retryResourceHistory,
		onRetryRuntimeHistory: retryRuntimeHistory,
	};
	return {
		resourceCapability,
		resourceQuery,
		resourceHistoryByMetric,
		resourceHistoryErrorByMetric,
		resourceHistoryFetchingByMetric,
		retryResourceHistory,
		runtimeHistoryByMetric,
		runtimeHistoryErrorByMetric,
		runtimeHistoryFetchingByMetric,
		retryRuntimeHistory,
		resourceTabProps,
	};
}
