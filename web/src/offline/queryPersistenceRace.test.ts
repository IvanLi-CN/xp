import { beforeEach, describe, expect, it, vi } from "vitest";

const stores = vi.hoisted(() => new Map<string, Map<string, unknown>>());
vi.mock("localforage", () => ({
	default: {
		createInstance: ({ storeName }: { storeName: string }) => {
			const data = new Map<string, unknown>();
			stores.set(storeName, data);
			return {
				getItem: async (key: string) => data.get(key) ?? null,
				setItem: async (key: string, value: unknown) => data.set(key, value),
				removeItem: async (key: string) => data.delete(key),
				clear: async () => data.clear(),
			};
		},
	},
}));

import {
	clearPersistedQueryCache,
	createPersistOptions,
	writePersistedQuerySnapshot,
} from "./queryPersistence";

describe("query cache cleanup", () => {
	beforeEach(async () => {
		vi.stubGlobal("__XP_WEB_BUILD_ID__", "test-build");
		await clearPersistedQueryCache();
	});

	it("does not restore queued shared or per-query writes after cleanup", async () => {
		const persister = createPersistOptions().persister;
		const write = persister.persistClient({
			buster: "test-build",
			timestamp: Date.now(),
			clientState: { mutations: [], queries: [] },
		});
		const snapshot = writePersistedQuerySnapshot(
			["adminNodes"],
			["old"],
			Date.now(),
		);
		const cleanup = clearPersistedQueryCache();
		await Promise.all([write, snapshot, cleanup]);
		expect(await persister.restoreClient()).toBeUndefined();
		expect(stores.get("offline_query_snapshots")?.size).toBe(0);
	});
});
