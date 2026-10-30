import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const stores = vi.hoisted(() => new Map<string, Map<string, unknown>>());
const reads = vi.hoisted(() => ({
	store: "",
	started: undefined as (() => void) | undefined,
	release: undefined as (() => void) | undefined,
	removeStarted: undefined as (() => void) | undefined,
	removeRelease: undefined as (() => void) | undefined,
	pauseRemove: false,
}));
vi.mock("localforage", () => ({
	default: {
		createInstance: ({ storeName }: { storeName: string }) => {
			const data = new Map<string, unknown>();
			stores.set(storeName, data);
			return {
				getItem: async (key: string) => {
					const value = data.get(key) ?? null;
					if (reads.store === storeName) {
						reads.store = "";
						await new Promise<void>((resolve) => {
							reads.release = resolve;
							reads.started?.();
						});
					}
					return value;
				},
				setItem: async (key: string, value: unknown) => data.set(key, value),
				removeItem: async (key: string) => {
					if (reads.pauseRemove && storeName === "offline_query_snapshots") {
						reads.pauseRemove = false;
						await new Promise<void>((resolve) => {
							reads.removeRelease = resolve;
							reads.removeStarted?.();
						});
					}
					return data.delete(key);
				},
				clear: async () => data.clear(),
			};
		},
	},
}));

import {
	clearPersistedQueryCache,
	createPersistOptions,
	readPersistedQuerySnapshot,
	writePersistedQuerySnapshot,
} from "./queryPersistence";

describe("query cache cleanup", () => {
	beforeEach(async () => {
		vi.stubGlobal("__XP_WEB_BUILD_ID__", "test-build");
		await clearPersistedQueryCache();
	});
	afterEach(() => {
		reads.store = "";
		reads.release?.();
		reads.release = undefined;
		reads.started = undefined;
		reads.pauseRemove = false;
		reads.removeRelease?.();
		reads.removeRelease = undefined;
		reads.removeStarted = undefined;
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});

	it("finishes stale snapshot removal before a new session writes the same key", async () => {
		const key = ["adminNodes"];
		stores.get("offline_query_snapshots")?.set(JSON.stringify(key), {
			data: ["old"],
			buildId: "test-build",
			storedAt: 1,
		});
		reads.pauseRemove = true;
		const started = new Promise<void>((resolve) => {
			reads.removeStarted = resolve;
		});
		const oldRead = readPersistedQuerySnapshot(key);
		await started;
		const cleanup = clearPersistedQueryCache();
		const newWrite = writePersistedQuerySnapshot(key, ["new"], Date.now());
		reads.removeRelease?.();
		await Promise.all([oldRead, cleanup, newWrite]);
		expect((await readPersistedQuerySnapshot<string[]>(key)).data).toEqual([
			"new",
		]);
	});

	it("keeps a newer snapshot written while an old read is pending", async () => {
		const key = ["adminNodes"];
		stores.get("offline_query_snapshots")?.set(JSON.stringify(key), {
			data: ["old"],
			buildId: "test-build",
			storedAt: 1,
		});
		reads.store = "offline_query_snapshots";
		const started = new Promise<void>((resolve) => {
			reads.started = resolve;
		});
		const oldRead = readPersistedQuerySnapshot(key);
		await started;
		await writePersistedQuerySnapshot(key, ["new"], Date.now());
		reads.release?.();
		await oldRead;
		expect((await readPersistedQuerySnapshot<string[]>(key)).data).toEqual([
			"new",
		]);
	});

	it.each(["offline_query_snapshots", "react_query_cache"])(
		"discards an in-flight read from %s when the session is cleared",
		async (storeName) => {
			const cache = {
				buster: "test-build",
				timestamp: Date.now(),
				clientState: { mutations: [], queries: [] },
			};
			stores
				.get("react_query_cache")
				?.set("xp-react-query", JSON.stringify(cache));
			stores
				.get("offline_query_snapshots")
				?.set(JSON.stringify(["adminNodes"]), {
					data: ["old"],
					dataUpdatedAt: Date.now(),
					storedAt: Date.now(),
					buildId: "test-build",
				});
			reads.store = storeName;
			const started = new Promise<void>((resolve) => {
				reads.started = resolve;
			});
			const pending =
				storeName === "react_query_cache"
					? createPersistOptions().persister.restoreClient()
					: readPersistedQuerySnapshot(["adminNodes"]);
			await started;
			const cleanup = clearPersistedQueryCache();
			reads.release?.();
			const restored = await pending;
			await cleanup;
			if (storeName === "react_query_cache") expect(restored).toBeUndefined();
			else expect(restored).toEqual({ data: undefined, dataUpdatedAt: null });
		},
	);

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
