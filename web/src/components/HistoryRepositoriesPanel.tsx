import { useQuery } from "@tanstack/react-query";

import { fetchAdminHistoryRepositories } from "../api/adminHistoryRepositories";
import { useApiCapability } from "../api/useApiCompatibility";
import { useAppRuntime } from "../offline/appRuntime";
import {
	formatSyncTimestamp,
	hasQueryData,
	queryIsOfflineBlocked,
} from "../offline/queryReadState";
import { useQueryWithOfflineFallback } from "../offline/useQueryWithOfflineFallback";
import { formatBackendError } from "../utils/backendErrorMessage";
import { Button } from "./Button";
import { HistoryRepositoryMembershipEditor } from "./HistoryRepositoryMembershipEditor";
import { RepositoryStatusSummary } from "./HistoryRepositoryStatus";
import { CapabilityUnavailableState, PageState } from "./PageState";
import { ReadStateBanner } from "./ReadStateBanner";

export function HistoryRepositoriesPanel(props: {
	adminToken: string;
	nodes: Array<{ node_id: string; node_name: string }>;
}) {
	const { adminToken } = props;
	const runtime = useAppRuntime();
	const capability = useApiCapability("admin.history-repositories");
	const query = useQuery({
		queryKey: ["adminHistoryRepositories", adminToken],
		enabled:
			adminToken.length > 0 && (capability.available || !runtime.isOnline),
		queryFn: ({ signal }) => fetchAdminHistoryRepositories(adminToken, signal),
	});
	const state = useQueryWithOfflineFallback(
		["adminHistoryRepositories", adminToken],
		query,
	);

	let content = null;
	if (capability.unavailable && !hasQueryData(state)) {
		content = (
			<CapabilityUnavailableState
				title="History repositories unavailable"
				reason={capability.reason}
			/>
		);
	} else if (state.isLoading && !hasQueryData(state)) {
		content = (
			<PageState
				variant="loading"
				title="Loading history repositories"
				description="Collecting repository lifecycle and capacity status."
			/>
		);
	} else if (
		!hasQueryData(state) &&
		queryIsOfflineBlocked(state, runtime.isOnline)
	) {
		content = (
			<PageState
				variant="offline"
				title="Offline repository status unavailable"
				description="Open Nodes while online to cache repository synchronization status."
			/>
		);
	} else if (state.isError && !hasQueryData(state)) {
		content = (
			<PageState
				variant="error"
				title="Failed to load history repositories"
				description={formatBackendError(state.error)}
				error={state.error}
				action={
					<Button
						variant="secondary"
						loading={state.isFetching}
						onClick={() => state.refetch()}
					>
						Retry
					</Button>
				}
			/>
		);
	} else if (state.data) {
		content = (
			<>
				<RepositoryStatusSummary status={state.data} />
				<HistoryRepositoryMembershipEditor
					adminToken={adminToken}
					members={state.data.items.map((item) => item.member)}
					nodes={props.nodes}
					disabled={
						runtime.isReadOnly || !runtime.isOnline || !capability.available
					}
				/>
			</>
		);
	}

	return (
		<div className="space-y-4">
			{state.data && (state.isError || !runtime.isOnline) ? (
				<ReadStateBanner
					tone={!runtime.isOnline ? "warning" : "info"}
					variant="inline"
					dismissible
					errors={state.isError ? [state.error] : []}
					title={
						!runtime.isOnline
							? "Offline repository status"
							: "Showing cached history repositories"
					}
					description={`Last successful sync: ${formatSyncTimestamp(state.dataUpdatedAt)}.`}
				/>
			) : null}
			{content}
		</div>
	);
}
