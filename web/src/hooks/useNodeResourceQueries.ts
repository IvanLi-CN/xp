import { useQuery } from "@tanstack/react-query";

import {
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
	const resourceHistoryQuery = useQuery({
		queryKey: ["adminNodeResourceHistory", props.adminToken, props.nodeId],
		enabled: enabled && props.activeTab === "resources",
		queryFn: ({ signal }) =>
			fetchAdminNodeResourceHistory(
				props.adminToken,
				props.nodeId,
				"cpu_busy_percent",
				signal,
			),
		refetchInterval: props.isOnline ? 30_000 : false,
	});
	return { resourceCapability, resourceQuery, resourceHistoryQuery };
}
