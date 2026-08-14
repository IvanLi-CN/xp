import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";

import { fetchAdminRepositoryHistory } from "../api/adminRepositoryHistory";
import { useApiCapability } from "../api/useApiCompatibility";
import { useAppRuntime } from "../offline/appRuntime";
import {
	formatSyncTimestamp,
	hasQueryData,
	queryIsOfflineBlocked,
} from "../offline/queryReadState";
import { useQueryWithOfflineFallback } from "../offline/useQueryWithOfflineFallback";
import { formatBackendError } from "../utils/backendErrorMessage";
import { RepositoryQueryQuality } from "./HistoryRepositoryStatus";
import { CapabilityUnavailableState, PageState } from "./PageState";
import { QueryErrorState } from "./QueryErrorState";
import { ReadStateBanner } from "./ReadStateBanner";

export function NodeRepositoryQuality(props: {
	adminToken: string;
	nodeId: string;
}) {
	const { adminToken, nodeId } = props;
	const runtime = useAppRuntime();
	const capability = useApiCapability("admin.repository-history");
	const rangeEnd = useMemo(() => Math.floor(Date.now() / 1000 / 300) * 300, []);
	const [page, setPage] = useState({
		nodeId,
		rangeEnd,
		cursor: undefined as string | undefined,
	});
	const pageCursor =
		page.nodeId === nodeId && page.rangeEnd === rangeEnd
			? page.cursor
			: undefined;
	useEffect(() => {
		setPage((current) =>
			current.nodeId === nodeId && current.rangeEnd === rangeEnd
				? current
				: { nodeId, rangeEnd, cursor: undefined },
		);
	}, [nodeId, rangeEnd]);
	const query = useQuery({
		queryKey: [
			"adminRepositoryHistory",
			adminToken,
			nodeId,
			rangeEnd,
			pageCursor,
		],
		enabled: adminToken.length > 0 && capability.available,
		queryFn: ({ signal }) =>
			fetchAdminRepositoryHistory(
				adminToken,
				{
					startUnixSeconds: rangeEnd - 24 * 60 * 60,
					endUnixSeconds: rangeEnd,
					pageSize: 100,
					pageCursor,
					subjectNodeId: nodeId,
				},
				signal,
			),
	});
	const state = useQueryWithOfflineFallback(
		["adminRepositoryHistory", adminToken, nodeId, rangeEnd, pageCursor],
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
	return state.data ? (
		<div className="space-y-3">
			{state.isError || !runtime.isOnline ? (
				<ReadStateBanner
					tone={!runtime.isOnline ? "warning" : "info"}
					variant="inline"
					dismissible
					errors={state.isError ? [state.error] : []}
					title={
						!runtime.isOnline
							? "Offline repository history"
							: "Showing cached repository history"
					}
					description={`Last successful sync: ${formatSyncTimestamp(state.dataUpdatedAt)}.`}
				/>
			) : null}
			<RepositoryQueryQuality
				history={state.data}
				onNextPage={
					state.data.next_page_cursor
						? () =>
								setPage({
									nodeId,
									rangeEnd,
									cursor: state.data?.next_page_cursor,
								})
						: undefined
				}
				nextPageLoading={state.isFetching}
			/>
		</div>
	) : null;
}
