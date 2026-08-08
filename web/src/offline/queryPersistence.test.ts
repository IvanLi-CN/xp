import { describe, expect, it } from "vitest";

import {
	isPersistedQueryCacheFresh,
	isPersistedQuerySnapshotFresh,
	mergePersistedQueries,
	shouldPersistQuery,
	snapshotStoredAt,
} from "./queryPersistence";

describe("persisted query snapshot freshness", () => {
	it("accepts a current-build snapshot within the 24-hour window", () => {
		expect(
			isPersistedQuerySnapshotFresh(
				{ buildId: "build-a", storedAt: 1000 },
				"build-a",
				1000 + 24 * 60 * 60 * 1000,
			),
		).toBe(true);
	});

	it("rejects expired and cross-build snapshots", () => {
		expect(
			isPersistedQuerySnapshotFresh(
				{ buildId: "build-a", storedAt: 1000 },
				"build-a",
				1001 + 24 * 60 * 60 * 1000,
			),
		).toBe(false);
		expect(
			isPersistedQuerySnapshotFresh(
				{ buildId: "build-a", storedAt: 1000 },
				"build-b",
				1000,
			),
		).toBe(false);
	});

	it("applies the same build and 24-hour boundary to the shared cache", () => {
		expect(
			isPersistedQueryCacheFresh(
				{ buster: "build-a", timestamp: 1000 },
				"build-a",
				1000 + 24 * 60 * 60 * 1000,
			),
		).toBe(true);
		expect(
			isPersistedQueryCacheFresh(
				{ buster: "build-a", timestamp: 1000 },
				"build-b",
				1000,
			),
		).toBe(false);
		expect(
			isPersistedQueryCacheFresh(
				{ buster: "build-a", timestamp: 1000 },
				"build-a",
				1001 + 24 * 60 * 60 * 1000,
			),
		).toBe(false);
	});

	it("does not renew stale omitted queries during cache merges", () => {
		const now = 1_000 + 24 * 60 * 60 * 1000;
		const merged = mergePersistedQueries(
			{
				buster: "build-a",
				timestamp: now,
				clientState: { queries: [] },
			},
			{
				buster: "build-a",
				timestamp: 1_000,
				clientState: {
					queries: [
						{
							queryKey: ["adminNodes"],
							state: { data: ["stale"], dataUpdatedAt: 1_000 },
						},
					],
				},
			},
		);
		expect(merged.clientState?.queries).toEqual([]);
	});

	it("keeps hydrated snapshots anchored to their data timestamp", () => {
		expect(snapshotStoredAt(1_000, 9_000)).toBe(1_000);
		expect(snapshotStoredAt(0, 9_000)).toBe(9_000);
	});

	it("persists node and user traffic reports", () => {
		expect(
			shouldPersistQuery(["adminNodeTraffic", "token", "node-1", "24h"]),
		).toBe(true);
		expect(
			shouldPersistQuery(["adminUserTraffic", "token", "user-1", "31d", null]),
		).toBe(true);
	});
});
