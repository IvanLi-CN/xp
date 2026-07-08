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

const basePersister = createAsyncStoragePersister({
	storage,
	key: PERSISTED_QUERY_STORAGE_KEY,
	throttleTime: 1_000,
});

const persister: PersistQueryClientProviderProps["persistOptions"]["persister"] =
	{
		persistClient: async (client) => {
			try {
				await basePersister.persistClient(client);
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

export async function readPersistedQuerySnapshot<T>(
	queryKey: readonly unknown[],
): Promise<{ data: T | undefined; dataUpdatedAt: number | null }> {
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

type PersistedQueryCache = {
	clientState?: {
		queries?: Array<{
			queryKey?: readonly unknown[];
			state?: {
				data?: unknown;
				dataUpdatedAt?: number;
			};
		}>;
	};
};
