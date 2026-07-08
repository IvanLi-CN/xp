import { useEffect, useState } from "react";

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

	useEffect(() => {
		if (hasQueryData(query)) {
			setFallback({ data: undefined, dataUpdatedAt: null });
			return;
		}

		let cancelled = false;
		void readPersistedQuerySnapshot<T>(queryKey).then((snapshot) => {
			if (cancelled) return;
			setFallback(snapshot);
		});

		return () => {
			cancelled = true;
		};
	}, [query, queryKey]);

	return {
		...query,
		data: query.data ?? fallback.data,
		dataUpdatedAt:
			query.dataUpdatedAt > 0
				? query.dataUpdatedAt
				: (fallback.dataUpdatedAt ?? 0),
	};
}
