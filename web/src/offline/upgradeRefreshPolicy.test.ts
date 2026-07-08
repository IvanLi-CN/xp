import { describe, expect, it } from "vitest";

import { shouldRefreshAfterUpgradeTransition } from "./upgradeRefreshPolicy";

describe("shouldRefreshAfterUpgradeTransition", () => {
	it("refreshes when upgrade moves from running to succeeded", () => {
		expect(shouldRefreshAfterUpgradeTransition("running", "succeeded")).toBe(
			true,
		);
	});

	it("refreshes when upgrade moves from restarting to failed", () => {
		expect(shouldRefreshAfterUpgradeTransition("restarting", "failed")).toBe(
			true,
		);
	});

	it("does not refresh while upgrade is still active", () => {
		expect(shouldRefreshAfterUpgradeTransition("running", "running")).toBe(
			false,
		);
		expect(
			shouldRefreshAfterUpgradeTransition("restarting", "restarting"),
		).toBe(false);
	});

	it("does not refresh on an initial terminal snapshot", () => {
		expect(shouldRefreshAfterUpgradeTransition(null, "succeeded")).toBe(false);
		expect(shouldRefreshAfterUpgradeTransition(null, "failed")).toBe(false);
	});
});
