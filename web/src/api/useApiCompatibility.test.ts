import { describe, expect, it } from "vitest";

import { resolveApiCompatibility } from "./apiCompatibility";
import { getApiCapabilityState } from "./useApiCompatibility";

describe("API capability state", () => {
	it("blocks protected queries while the compatibility probe is pending", () => {
		const state = getApiCapabilityState(null, "admin.nodes");

		expect(state).toMatchObject({
			available: false,
			unavailable: true,
			pending: true,
		});
		expect(state.reason).toMatch(/Checking API compatibility/);
	});

	it("degrades only the capability missing from a compatible profile", () => {
		const compatibility = resolveApiCompatibility({
			releaseTag: "v3.21.11",
			capabilities: ["api.health", "api.cluster-info", "admin.nodes"],
		});
		const state = getApiCapabilityState(compatibility, "admin.mesh");

		expect(state.available).toBe(false);
		expect(state.pending).toBe(false);
		expect(state.reason).toMatch(/admin\.mesh/);
	});
});
