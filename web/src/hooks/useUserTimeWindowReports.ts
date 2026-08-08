import { useQuery } from "@tanstack/react-query";

import {
	type AdminIpUsageWindow,
	fetchAdminUserIpUsage,
} from "../api/adminIpUsage";
import { type TrafficWindow, fetchAdminUserTraffic } from "../api/adminTraffic";
import {
	alignUserIpUsageResponse,
	alignUserTrafficResponse,
	emptyUserIpUsageResponse,
	emptyUserTrafficResponse,
} from "../utils/timeWindowReports";
import { useTimeWindowTransition } from "./useTimeWindowTransition";

type UseUserTimeWindowReportsOptions = {
	adminToken: string;
	ipUsageEnabled: boolean;
	ipUsageWindow: AdminIpUsageWindow;
	nodeId: string | null;
	trafficEnabled: boolean;
	trafficWindow: TrafficWindow;
	userId: string;
};

export function useUserTimeWindowReports({
	adminToken,
	ipUsageEnabled,
	ipUsageWindow,
	nodeId,
	trafficEnabled,
	trafficWindow,
	userId,
}: UseUserTimeWindowReportsOptions) {
	const ipUsageQuery = useQuery({
		queryKey: ["adminUserIpUsage", adminToken, userId, ipUsageWindow],
		enabled: ipUsageEnabled,
		queryFn: ({ signal }) =>
			fetchAdminUserIpUsage(adminToken, userId, ipUsageWindow, signal),
	});
	const trafficQuery = useQuery({
		queryKey: ["adminUserTraffic", adminToken, userId, trafficWindow, nodeId],
		enabled: trafficEnabled,
		queryFn: ({ signal }) =>
			fetchAdminUserTraffic(adminToken, userId, trafficWindow, nodeId, signal),
	});

	return {
		ipUsageQuery,
		ipUsageDisplay: useTimeWindowTransition({
			alignData: alignUserIpUsageResponse,
			createEmptyData: emptyUserIpUsageResponse,
			data: ipUsageQuery.data,
			dataUpdatedAt: ipUsageQuery.dataUpdatedAt,
			identity: `user-ip-usage:${userId}`,
			isError: ipUsageQuery.isError,
			isFetchedAfterMount: ipUsageQuery.isFetchedAfterMount,
			isFetching: ipUsageQuery.isFetching,
			window: ipUsageWindow,
		}),
		trafficQuery,
		trafficDisplay: useTimeWindowTransition({
			alignData: alignUserTrafficResponse,
			createEmptyData: emptyUserTrafficResponse,
			data: trafficQuery.data,
			dataUpdatedAt: trafficQuery.dataUpdatedAt,
			identity: `user-traffic:${userId}:${nodeId ?? "all"}`,
			isError: trafficQuery.isError,
			isFetchedAfterMount: trafficQuery.isFetchedAfterMount,
			isFetching: trafficQuery.isFetching,
			window: trafficWindow,
		}),
	};
}
