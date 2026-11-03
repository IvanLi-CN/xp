import { createAsyncStoragePersister } from "@tanstack/query-async-storage-persister";
import type { QueryClient } from "@tanstack/react-query";
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
	"adminNodeTraffic",
	"adminNodeTcpConnections",
	"adminQuotaPolicyGlobalWeightRows",
	"adminQuotaPolicyNodePolicy",
	"adminQuotaPolicyNodeWeightRows",
	"adminUpgradeStatus",
	"adminUser",
	"adminUserIpUsage",
	"adminUserTraffic",
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

let persistenceEpoch = 0;
let persistenceQueue: Promise<void> = Promise.resolve();
let pendingClient: { epoch: number; client: PersistedQueryCache } | null = null;
let persistTimer: ReturnType<typeof setTimeout> | null = null;

function enqueuePersistence(operation: () => Promise<void>): Promise<void> {
	const next = persistenceQueue.then(operation, operation);
	persistenceQueue = next.catch(() => undefined);
	return next;
}

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

export function mergePersistedQueries(
	current: PersistedQueryCache,
	previous: PersistedQueryCache | undefined,
): PersistedQueryCache {
	if (!previous || previous.buster !== current.buster) return current;
	const currentQueries = current.clientState?.queries ?? [];
	const currentIds = new Set(currentQueries.map(queryIdentifier));
	const oldestAllowed = Date.now() - DAY_MS;
	const preservedQueries = (previous.clientState?.queries ?? []).filter(
		(query) =>
			!currentIds.has(queryIdentifier(query)) &&
			typeof query.state?.dataUpdatedAt === "number" &&
			query.state.dataUpdatedAt >= oldestAllowed,
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
		persistClient: (client) => {
			pendingClient = {
				epoch: persistenceEpoch,
				client: client as PersistedQueryCache,
			};
			if (persistTimer !== null) return;
			persistTimer = setTimeout(() => {
				persistTimer = null;
				const pending = pendingClient;
				pendingClient = null;
				if (!pending) return;
				void enqueuePersistence(async () => {
					const { epoch, client } = pending;
					if (epoch !== persistenceEpoch) return;
					const previous = (await basePersister.restoreClient()) as
						| PersistedQueryCache
						| undefined;
					if (epoch !== persistenceEpoch) return;
					await storage.setItem(
						PERSISTED_QUERY_STORAGE_KEY,
						JSON.stringify(mergePersistedQueries(client, previous)),
					);
				}).catch(() => undefined);
			}, 1_000);
		},
		restoreClient: async () => {
			const epoch = persistenceEpoch;
			try {
				await persistenceQueue;
				if (epoch !== persistenceEpoch) return undefined;
				const restored = await basePersister.restoreClient();
				return epoch === persistenceEpoch ? restored : undefined;
			} catch {
				return undefined;
			}
		},
		removeClient: async () => {
			await clearPersistedQueryCache();
		},
	};

export function shouldPersistQuery(queryKey: readonly unknown[]) {
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

export async function clearPersistedQueryCache(): Promise<void> {
	persistenceEpoch += 1;
	if (persistTimer !== null) clearTimeout(persistTimer);
	persistTimer = null;
	pendingClient = null;
	await enqueuePersistence(async () => {
		try {
			await basePersister.removeClient();
		} catch {
			// Offline persistence is an enhancement, not a hard dependency.
		}
		try {
			await offlineSnapshotStorage.clear();
		} catch {
			// Offline persistence is an enhancement, not a hard dependency.
		}
	});
}

export function clearBackendQueryCache(queryClient: QueryClient): void {
	queryClient.removeQueries();
	void clearPersistedQueryCache();
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

export function snapshotStoredAt(
	dataUpdatedAt: number,
	now = Date.now(),
): number {
	return dataUpdatedAt > 0 ? dataUpdatedAt : now;
}

export function isPersistedQueryCacheFresh(
	cache: Pick<PersistedQueryCache, "buster" | "timestamp">,
	buildId: string,
	now = Date.now(),
): boolean {
	return (
		cache.buster === buildId &&
		typeof cache.timestamp === "number" &&
		now - cache.timestamp <= DAY_MS
	);
}

export async function readPersistedQuerySnapshot<T>(
	queryKey: readonly unknown[],
): Promise<{ data: T | undefined; dataUpdatedAt: number | null }> {
	const epoch = persistenceEpoch;
	const empty = { data: undefined, dataUpdatedAt: null };
	await persistenceQueue;
	if (epoch !== persistenceEpoch) return empty;
	try {
		const snapshotKey = JSON.stringify(queryKey);
		const snapshot = await offlineSnapshotStorage.getItem<{
			data?: T;
			dataUpdatedAt?: number;
			buildId?: string;
			storedAt?: number;
		}>(snapshotKey);
		if (epoch !== persistenceEpoch) return empty;
		if (
			snapshot &&
			isPersistedQuerySnapshotFresh(snapshot, __XP_WEB_BUILD_ID__)
		) {
			return {
				data: snapshot.data,
				dataUpdatedAt: snapshot.dataUpdatedAt ?? null,
			};
		}
		// Other tabs may write this key between an IndexedDB read and delete.
		// Ignore stale data; a fresh write or session cleanup replaces it safely.
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
	if (epoch !== persistenceEpoch) return empty;
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
	if (!parsed || !isPersistedQueryCacheFresh(parsed, __XP_WEB_BUILD_ID__)) {
		return { data: undefined, dataUpdatedAt: null };
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
	const epoch = persistenceEpoch;
	try {
		await enqueuePersistence(async () => {
			if (epoch !== persistenceEpoch) return;
			await offlineSnapshotStorage.setItem(JSON.stringify(queryKey), {
				data,
				dataUpdatedAt,
				buildId: __XP_WEB_BUILD_ID__,
				storedAt: snapshotStoredAt(dataUpdatedAt),
			});
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
