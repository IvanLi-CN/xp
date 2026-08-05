import { afterEach, describe, expect, it, vi } from "vitest";

import {
	API_COMPATIBILITY_WINDOW,
	resolveApiCompatibility,
	resolveReleaseMinor,
} from "./apiCompatibility";

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("api compatibility", () => {
	it("accepts only stable release tags inside the pinned window", () => {
		expect(resolveReleaseMinor("v3.22.5")).toBe("3.22");
		expect(resolveReleaseMinor("v3.21.11")).toBe("3.21");
		expect(resolveReleaseMinor("v3.20.3")).toBe("3.20");
		expect(resolveReleaseMinor("v3.23.0-rc.1")).toBeNull();
		expect(resolveReleaseMinor("main")).toBeNull();
		expect(resolveReleaseMinor("v2.99.1")).toBeNull();
	});

	it("selects a profile from capabilities without using the web build id", () => {
		const result = resolveApiCompatibility({
			capabilities: [
				"api.health",
				"api.cluster-info",
				"admin.nodes",
				"admin.status-events",
			],
		});

		expect(result.kind).toBe("compatible");
		if (result.kind !== "compatible") return;
		expect(API_COMPATIBILITY_WINDOW).toContain(result.profile.minor);
		expect(result.profile.minor).not.toContain("2026.08.05-new");
	});

	it("reports local feature degradation for a missing capability", () => {
		const result = resolveApiCompatibility({
			releaseTag: "v3.22.5",
			capabilities: ["api.health", "api.cluster-info", "admin.nodes"],
		});

		expect(result.kind).toBe("compatible");
		if (result.kind !== "compatible") return;
		expect(result.isFeatureAvailable("admin.status-events")).toBe(false);
		expect(result.degradationFor("admin.status-events")).toMatch(
			/API capability is unavailable/,
		);
	});

	it("rejects invalid and ambiguous fingerprints", () => {
		expect(
			resolveApiCompatibility({
				fingerprint: {
					"/api/health": ["status"],
					"/api/cluster/info": ["cluster_id"],
				},
			}).kind,
		).toBe("incompatible");
	});

	it("falls back from the additive probe to the strict release endpoint", async () => {
		const fetchMock = vi
			.fn()
			.mockResolvedValueOnce(new Response("not found", { status: 404 }))
			.mockResolvedValueOnce(
				new Response(
					JSON.stringify({
						current: { package: "3.21.11", release_tag: "v3.21.11" },
						latest: { release_tag: "v3.22.5" },
						has_update: true,
						checked_at: "2026-08-05T00:00:00Z",
						compare_reason: "semver",
						source: {
							kind: "github-releases",
							repo: "IvanLi-CN/xp",
							api_base: "https://api.github.com",
							channel: "stable",
						},
					}),
					{ status: 200 },
				),
			);
		vi.stubGlobal("fetch", fetchMock);

		const { fetchApiCompatibility } = await import("./apiCompatibility");
		const result = await fetchApiCompatibility();

		expect(result.kind).toBe("compatible");
		if (result.kind === "compatible") expect(result.profile.minor).toBe("3.21");
		expect(fetchMock).toHaveBeenNthCalledWith(
			2,
			"/api/version/check",
			expect.objectContaining({ method: "GET" }),
		);
	});
});
