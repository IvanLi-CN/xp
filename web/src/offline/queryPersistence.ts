import { createAsyncStoragePersister } from "@tanstack/query-async-storage-persister";
import type { PersistQueryClientProviderProps } from "@tanstack/react-query-persist-client";
import localforage from "localforage";

const DAY_MS = 24 * 60 * 60 * 1000;
const PERSISTED_QUERY_STORAGE_KEY = "xp-react-query";
const PERSISTED_QUERY_ROOTS = new Set([
	"health",
	"clusterInfo",
	"adminAlerts",
	"adminConfig",
	"adminEndpoint",
	"adminEndpoints",
	"adminNode",
	"adminNodes",
	"adminNodesRuntime",
	"adminNodeRuntime",
	"adminNodeHistory",
	"adminNodeIpUsage",
	"adminNodeTcpConnections",
	"adminQuotaPolicyGlobalWeightRows",
	"adminQuotaPolicyNodePolicy",
	"adminQuotaPolicyNodeWeightRows",
	"adminUpgradeStatus",
	"adminUser",
	"adminUserIpUsage",
	"adminUserMihomoProfile",
	"adminUsers",
	"adminUserAccess",
	"adminUserNodeQuotaStatus",
	"adminUserNodeQuotas",
	"adminUserNodeWeights",
	"adminUserQuotaSummaries",
	"versionCheck",
]);

const storage = localforage.createInstance({
	name: "xp",
	storeName: "react_query_cache",
	description: "Persisted read-only cache for offline admin access.",
});
const offlineSnapshotStorage = localforage.createInstance({
	name: "xp",
	storeName: "offline_query_snapshots",
	description: "Durable per-query snapshots for offline startup recovery.",
});

const basePersister = createAsyncStoragePersister({
	storage,
	key: PERSISTED_QUERY_STORAGE_KEY,
	throttleTime: 1_000,
});

type PersistedQuery = {
	queryHash?: string;
	queryKey?: readonly unknown[];
	state?: {
		data?: unknown;
		dataUpdatedAt?: number;
	};
};

function queryIdentifier(query: PersistedQuery) {
	return query.queryHash ?? JSON.stringify(query.queryKey);
}

function mergePersistedQueries(
	current: PersistedQueryCache,
	previous: PersistedQueryCache | undefined,
): PersistedQueryCache {
	if (!previous || previous.buster !== current.buster) return current;
	const currentQueries = current.clientState?.queries ?? [];
	const currentIds = new Set(currentQueries.map(queryIdentifier));
	const preservedQueries = (previous.clientState?.queries ?? []).filter(
		(query) => !currentIds.has(queryIdentifier(query)),
	);
	return {
		...current,
		clientState: {
			...current.clientState,
			queries: [...preservedQueries, ...currentQueries],
		},
	};
}

const persister: PersistQueryClientProviderProps["persistOptions"]["persister"] =
	{
		persistClient: async (client) => {
			try {
				const previous = (await basePersister.restoreClient()) as
					| PersistedQueryCache
					| undefined;
				await basePersister.persistClient(
					mergePersistedQueries(
						client as PersistedQueryCache,
						previous,
					) as Parameters<typeof basePersister.persistClient>[0],
				);
			} catch {
				// Some test and private-browsing environments do not expose IndexedDB.
				// Offline persistence should degrade silently instead of crashing the app.
			}
		},
		restoreClient: async () => {
			try {
				return await basePersister.restoreClient();
			} catch {
				return undefined;
			}
		},
		removeClient: async () => {
			try {
				await basePersister.removeClient();
			} catch {
				// Ignore cleanup failures when persistence is unavailable.
			}
		},
	};

function shouldPersistQuery(queryKey: readonly unknown[]) {
	const root = queryKey[0];
	return typeof root === "string" && PERSISTED_QUERY_ROOTS.has(root);
}

export function createPersistOptions(): PersistQueryClientProviderProps["persistOptions"] {
	return {
		persister,
		maxAge: DAY_MS,
		buster: __XP_WEB_BUILD_ID__,
		dehydrateOptions: {
			shouldDehydrateMutation: () => false,
			shouldDehydrateQuery: (query) =>
				query.state.status === "success" && shouldPersistQuery(query.queryKey),
		},
	};
}

type PersistedQuerySnapshot = {
	buildId?: string;
	storedAt?: number;
};

export function isPersistedQuerySnapshotFresh(
	snapshot: PersistedQuerySnapshot,
	buildId: string,
	now = Date.now(),
): boolean {
	return (
		snapshot.buildId === buildId &&
		typeof snapshot.storedAt === "number" &&
		now - snapshot.storedAt <= DAY_MS
	);
}

export async function readPersistedQuerySnapshot<T>(
	queryKey: readonly unknown[],
): Promise<{ data: T | undefined; dataUpdatedAt: number | null }> {
	try {
		const snapshotKey = JSON.stringify(queryKey);
		const snapshot = await offlineSnapshotStorage.getItem<{
			data?: T;
			dataUpdatedAt?: number;
			buildId?: string;
			storedAt?: number;
		}>(snapshotKey);
		if (
			snapshot &&
			isPersistedQuerySnapshotFresh(snapshot, __XP_WEB_BUILD_ID__)
		) {
			return {
				data: snapshot.data,
				dataUpdatedAt: snapshot.dataUpdatedAt ?? null,
			};
		}
		if (snapshot) await offlineSnapshotStorage.removeItem(snapshotKey);
	} catch {
		// Fall through to the shared persisted query cache.
	}

	let raw: string | PersistedQueryCache | null;
	try {
		raw = await storage.getItem<string | PersistedQueryCache | null>(
			PERSISTED_QUERY_STORAGE_KEY,
		);
	} catch {
		return { data: undefined, dataUpdatedAt: null };
	}
	if (!raw) {
		return { data: undefined, dataUpdatedAt: null };
	}

	let parsed: PersistedQueryCache | null = null;
	if (typeof raw === "string") {
		try {
			parsed = JSON.parse(raw) as PersistedQueryCache;
		} catch {
			return { data: undefined, dataUpdatedAt: null };
		}
	} else {
		parsed = raw;
	}

	const target = JSON.stringify(queryKey);
	const match = parsed?.clientState?.queries?.find(
		(query) => JSON.stringify(query.queryKey) === target,
	);
	if (!match) {
		return { data: undefined, dataUpdatedAt: null };
	}

	return {
		data: match.state?.data as T | undefined,
		dataUpdatedAt:
			typeof match.state?.dataUpdatedAt === "number"
				? match.state.dataUpdatedAt
				: null,
	};
}

export async function writePersistedQuerySnapshot<T>(
	queryKey: readonly unknown[],
	data: T,
	dataUpdatedAt: number,
): Promise<void> {
	try {
		await offlineSnapshotStorage.setItem(JSON.stringify(queryKey), {
			data,
			dataUpdatedAt,
			buildId: __XP_WEB_BUILD_ID__,
			storedAt: Date.now(),
		});
	} catch {
		// Offline persistence is an enhancement, not a hard dependency.
	}
}

type PersistedQueryCache = {
	buster?: string;
	timestamp?: number;
	clientState?: {
		mutations?: unknown[];
		queries?: PersistedQuery[];
	};
};
