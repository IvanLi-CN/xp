import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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
	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});

	it("does not restore queued shared or per-query writes after cleanup", async () => {
		vi.useFakeTimers();
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
		await vi.advanceTimersByTimeAsync(1_500);
		expect(await persister.restoreClient()).toBeUndefined();
		expect(stores.get("offline_query_snapshots")?.size).toBe(0);
	});

	it("coalesces rapid cache updates and persists the latest", async () => {
		vi.useFakeTimers();
		const persister = createPersistOptions().persister;
		await persister.persistClient({
			buster: "test-build",
			timestamp: 1,
			clientState: { mutations: [], queries: [] },
		});
		await persister.persistClient({
			buster: "test-build",
			timestamp: 2,
			clientState: { mutations: [], queries: [] },
		});
		await vi.advanceTimersByTimeAsync(1_000);
		expect((await persister.restoreClient())?.timestamp).toBe(2);
	});
});
