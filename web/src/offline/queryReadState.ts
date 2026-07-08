type QueryLike<T = unknown> = {
	data: T | undefined;
	dataUpdatedAt: number;
	fetchStatus?: "fetching" | "paused" | "idle";
	isError: boolean;
	isLoading: boolean;
};

export function hasQueryData<T>(query: QueryLike<T>) {
	return query.data !== undefined && query.data !== null;
}

export function queryIsOfflineBlocked<T>(
	query: QueryLike<T>,
	isOnline: boolean,
) {
	return !isOnline || query.fetchStatus === "paused";
}

export function latestQueryDataUpdatedAt(
	queries: Array<QueryLike<unknown> | null | undefined>,
) {
	let latest = 0;
	for (const query of queries) {
		if (!query || !hasQueryData(query) || query.dataUpdatedAt <= 0) continue;
		latest = Math.max(latest, query.dataUpdatedAt);
	}
	return latest > 0 ? latest : null;
}

export function formatSyncTimestamp(value: number | null | undefined) {
	if (!value || !Number.isFinite(value) || value <= 0) return "unknown";
	return new Date(value).toLocaleString();
}
