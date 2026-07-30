import { useEffect, useMemo, useState } from "react";

import { readPersistedQuerySnapshot } from "./queryPersistence";
import { hasQueryData } from "./queryReadState";

type QueryLike<T> = {
	data: T | undefined;
	dataUpdatedAt: number;
	isError: boolean;
	isLoading: boolean;
	fetchStatus?: "fetching" | "paused" | "idle";
};

type PersistedSnapshot<T> = {
	data: T | undefined;
	dataUpdatedAt: number | null;
};

export function useQueryWithOfflineFallback<T, TQuery extends QueryLike<T>>(
	queryKey: readonly unknown[],
	query: TQuery,
) {
	const [fallback, setFallback] = useState<PersistedSnapshot<T>>({
		data: undefined,
		dataUpdatedAt: null,
	});
	const queryKeyHash = JSON.stringify(queryKey);
	const persistedQueryKey = useMemo(
		() => JSON.parse(queryKeyHash) as readonly unknown[],
		[queryKeyHash],
	);
	const queryHasData = hasQueryData(query);

	useEffect(() => {
		if (queryHasData) {
			setFallback((current) =>
				current.data === undefined && current.dataUpdatedAt === null
					? current
					: { data: undefined, dataUpdatedAt: null },
			);
			return;
		}

		let cancelled = false;
		void readPersistedQuerySnapshot<T>(persistedQueryKey).then((snapshot) => {
			if (cancelled) return;
			setFallback(snapshot);
		});

		return () => {
			cancelled = true;
		};
	}, [persistedQueryKey, queryHasData]);

	return {
		...query,
		data: query.data ?? fallback.data,
		dataUpdatedAt:
			query.dataUpdatedAt > 0
				? query.dataUpdatedAt
				: (fallback.dataUpdatedAt ?? 0),
	};
}
