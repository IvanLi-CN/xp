import { describe, expect, it } from "vitest";

import { resolveApiCompatibility } from "./apiCompatibility";
import {
	getApiCapabilityState,
	getApiRouteState,
	persistCompatibility,
	readPersistedCompatibility,
} from "./useApiCompatibility";

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

	it("does not expose a write route absent from the selected server profile", () => {
		const compatibility = resolveApiCompatibility({
			releaseTag: "v3.22.5",
		});
		const state = getApiRouteState(
			compatibility,
			"admin.mesh",
			"PUT /api/admin/mesh/config",
		);

		expect(state.available).toBe(false);
		expect(state.reason).toMatch(/PUT \/api\/admin\/mesh\/config/);
	});

	it("does not infer the current write route from legacy mesh telemetry", () => {
		const compatibility = resolveApiCompatibility({
			releaseTag: "v3.22.5",
			capabilities: ["admin.mesh", "admin.mesh-transport-reuse"],
			capabilitiesAreAuthoritative: true,
		});
		const state = getApiRouteState(
			compatibility,
			"admin.mesh",
			"PUT /api/admin/mesh/config",
		);

		expect(state.available).toBe(false);
		expect(state.unavailable).toBe(true);
	});

	it("accepts current routes only with a current route contract capability", () => {
		const compatibility = resolveApiCompatibility({
			releaseTag: "v0.2.0",
			capabilities: ["admin.mesh", "cluster.mesh-gate-v1"],
			capabilitiesAreAuthoritative: true,
		});
		const state = getApiRouteState(
			compatibility,
			"admin.mesh",
			"PUT /api/admin/mesh/config",
		);

		expect(state.available).toBe(true);
		expect(state.unavailable).toBe(false);
	});

	it("keeps probe-only capabilities disabled after persistence and restore", () => {
		const compatibility = resolveApiCompatibility({
			releaseTag: "v3.22.5",
		});
		persistCompatibility(
			"cache-test",
			compatibility,
			"https://primary.example",
		);

		const restored = readPersistedCompatibility(
			"cache-test",
			"https://primary.example",
		);
		expect(restored?.kind).toBe("compatible");
		if (!restored || restored.kind !== "compatible") return;
		expect(
			restored.isFeatureAvailable("admin.mihomo-subscription-import"),
		).toBe(false);
	});

	it("does not restore another backend's persisted capability snapshot", () => {
		const compatibility = resolveApiCompatibility({
			releaseTag: "v3.22.5",
			capabilities: ["api.health", "admin.mihomo-subscription-import"],
		});
		persistCompatibility(
			"origin-test",
			compatibility,
			"https://primary.example",
		);

		expect(
			readPersistedCompatibility("origin-test", "https://secondary.example"),
		).toBeUndefined();
	});
});
