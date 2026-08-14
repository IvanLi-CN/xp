import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";

import { fetchAdminRepositoryHistory } from "../api/adminRepositoryHistory";
import { useApiCapability } from "../api/useApiCompatibility";
import { useAppRuntime } from "../offline/appRuntime";
import { hasQueryData, queryIsOfflineBlocked } from "../offline/queryReadState";
import { useQueryWithOfflineFallback } from "../offline/useQueryWithOfflineFallback";
import { formatBackendError } from "../utils/formatBackendError";
import { RepositoryQueryQuality } from "./HistoryRepositoryStatus";
import { CapabilityUnavailableState, PageState } from "./PageState";
import { QueryErrorState } from "./QueryErrorState";

export function NodeRepositoryHistoryQuality(props: {
	adminToken: string;
	nodeId: string;
}) {
	const { adminToken, nodeId } = props;
	const runtime = useAppRuntime();
	const capability = useApiCapability("admin.repository-history");
	const rangeEnd = useMemo(() => Math.floor(Date.now() / 1000 / 300) * 300, []);
	const query = useQuery({
		queryKey: ["adminRepositoryHistory", adminToken, nodeId, rangeEnd],
		enabled: adminToken.length > 0 && capability.available,
		queryFn: ({ signal }) =>
			fetchAdminRepositoryHistory(
				adminToken,
				{
					startUnixSeconds: rangeEnd - 24 * 60 * 60,
					endUnixSeconds: rangeEnd,
					pageSize: 100,
				},
				signal,
			),
	});
	const state = useQueryWithOfflineFallback(
		["adminRepositoryHistory", adminToken, nodeId, rangeEnd],
		query,
	);

	if (capability.unavailable && !hasQueryData(state)) {
		return (
			<CapabilityUnavailableState
				title="Repository history unavailable"
				reason={capability.reason}
			/>
		);
	}
	if (state.isLoading && !hasQueryData(state)) {
		return (
			<PageState
				variant="loading"
				title="Loading repository history quality"
				description="Checking repository coverage and synchronization watermarks."
			/>
		);
	}
	if (!hasQueryData(state) && queryIsOfflineBlocked(state, runtime.isOnline)) {
		return (
			<PageState
				variant="offline"
				title="Offline repository history unavailable"
				description="Connect once to cache repository completeness information."
			/>
		);
	}
	if (state.isError && !hasQueryData(state)) {
		return (
			<QueryErrorState
				title="Failed to load repository history quality"
				description={formatBackendError(state.error)}
				error={state.error}
				loading={state.isFetching}
				onRetry={() => state.refetch()}
			/>
		);
	}
	return state.data ? <RepositoryQueryQuality history={state.data} /> : null;
}
