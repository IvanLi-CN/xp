import { describe, expect, it } from "vitest";

import { isPersistedQuerySnapshotFresh } from "./queryPersistence";

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
});
