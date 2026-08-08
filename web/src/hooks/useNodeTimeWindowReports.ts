import { useQuery } from "@tanstack/react-query";

import {
	type AdminIpUsageWindow,
	fetchAdminNodeIpUsage,
} from "../api/adminIpUsage";
import {
	type AdminTcpConnectionUsageWindow,
	fetchAdminNodeTcpConnections,
} from "../api/adminTcpConnections";
import { type TrafficWindow, fetchAdminNodeTraffic } from "../api/adminTraffic";
import {
	alignNodeIpUsageResponse,
	alignNodeTcpConnectionsResponse,
	alignNodeTrafficResponse,
	emptyNodeIpUsageResponse,
	emptyNodeTcpConnectionsResponse,
	emptyNodeTrafficResponse,
} from "../utils/timeWindowReports";
import { useTimeWindowTransition } from "./useTimeWindowTransition";

type UseNodeTimeWindowReportsOptions = {
	adminToken: string;
	ipUsageEnabled: boolean;
	ipUsageWindow: AdminIpUsageWindow;
	nodeId: string;
	tcpConnectionsEnabled: boolean;
	tcpConnectionsWindow: AdminTcpConnectionUsageWindow;
	trafficEnabled: boolean;
	trafficWindow: TrafficWindow;
};

export function useNodeTimeWindowReports({
	adminToken,
	ipUsageEnabled,
	ipUsageWindow,
	nodeId,
	tcpConnectionsEnabled,
	tcpConnectionsWindow,
	trafficEnabled,
	trafficWindow,
}: UseNodeTimeWindowReportsOptions) {
	const trafficQuery = useQuery({
		queryKey: ["adminNodeTraffic", adminToken, nodeId, trafficWindow],
		enabled: trafficEnabled,
		queryFn: ({ signal }) =>
			fetchAdminNodeTraffic(adminToken, nodeId, trafficWindow, signal),
	});
	const ipUsageQuery = useQuery({
		queryKey: ["adminNodeIpUsage", adminToken, nodeId, ipUsageWindow],
		enabled: ipUsageEnabled,
		queryFn: ({ signal }) =>
			fetchAdminNodeIpUsage(adminToken, nodeId, ipUsageWindow, signal),
	});
	const tcpConnectionsQuery = useQuery({
		queryKey: [
			"adminNodeTcpConnections",
			adminToken,
			nodeId,
			tcpConnectionsWindow,
		],
		enabled: tcpConnectionsEnabled,
		queryFn: ({ signal }) =>
			fetchAdminNodeTcpConnections(
				adminToken,
				nodeId,
				tcpConnectionsWindow,
				signal,
			),
	});

	return {
		trafficQuery,
		trafficDisplay: useTimeWindowTransition({
			alignData: alignNodeTrafficResponse,
			createEmptyData: emptyNodeTrafficResponse,
			data: trafficQuery.data,
			dataUpdatedAt: trafficQuery.dataUpdatedAt,
			identity: `node-traffic:${nodeId}`,
			isError: trafficQuery.isError,
			isFetching: trafficQuery.isFetching,
			window: trafficWindow,
		}),
		ipUsageQuery,
		ipUsageDisplay: useTimeWindowTransition({
			alignData: alignNodeIpUsageResponse,
			createEmptyData: emptyNodeIpUsageResponse,
			data: ipUsageQuery.data,
			dataUpdatedAt: ipUsageQuery.dataUpdatedAt,
			identity: `node-ip-usage:${nodeId}`,
			isError: ipUsageQuery.isError,
			isFetching: ipUsageQuery.isFetching,
			window: ipUsageWindow,
		}),
		tcpConnectionsQuery,
		tcpConnectionsDisplay: useTimeWindowTransition({
			alignData: alignNodeTcpConnectionsResponse,
			createEmptyData: emptyNodeTcpConnectionsResponse,
			data: tcpConnectionsQuery.data,
			dataUpdatedAt: tcpConnectionsQuery.dataUpdatedAt,
			identity: `node-tcp-connections:${nodeId}`,
			isError: tcpConnectionsQuery.isError,
			isFetching: tcpConnectionsQuery.isFetching,
			window: tcpConnectionsWindow,
		}),
	};
}
