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

	it("rejects an explicitly empty capability advertisement", () => {
		expect(
			resolveApiCompatibility({
				releaseTag: "v3.22.5",
				capabilities: [],
			}),
		).toEqual({
			kind: "incompatible",
			reason: expect.stringContaining("empty API capability"),
		});
	});

	it("uses valid capabilities before an invalid release tag", () => {
		const result = resolveApiCompatibility({
			releaseTag: "v0.2.0",
			capabilities: [
				"api.health",
				"api.cluster-info",
				"admin.nodes",
				"admin.status-events",
			],
		});

		expect(result.kind).toBe("compatible");
	});

	it("preserves additive capabilities while selecting a stable profile", () => {
		const result = resolveApiCompatibility({
			capabilities: [
				"api.health",
				"api.cluster-info",
				"admin.nodes",
				"admin.status-events",
				"admin.endpoint-mihomo-smux",
			],
		});

		expect(result.kind).toBe("compatible");
		if (result.kind !== "compatible") return;
		expect(result.isFeatureAvailable("admin.endpoint-mihomo-smux")).toBe(true);
	});

	it("rejects incomplete fingerprints", () => {
		expect(
			resolveApiCompatibility({
				fingerprint: {
					"/api/health": ["status"],
					"/api/cluster/info": ["cluster_id"],
				},
			}).kind,
		).toBe("incompatible");
	});

	it("resolves equivalent legacy fingerprints without guessing a mixed build", () => {
		const result = resolveApiCompatibility({
			fingerprint: {
				"/api/health": ["status"],
				"/api/cluster/info": [
					"cluster_id",
					"node_id",
					"role",
					"leader_api_base_url",
					"term",
				],
				"/api/admin/nodes": ["items"],
			},
		});

		expect(result.kind).toBe("compatible");
		if (result.kind === "compatible") {
			expect(["3.21", "3.20"]).toContain(result.profile.minor);
		}
	});

	it("selects the newest fingerprint profile when its mesh contract is present", () => {
		const result = resolveApiCompatibility({
			fingerprint: {
				"/api/health": ["status"],
				"/api/cluster/info": [
					"cluster_id",
					"node_id",
					"role",
					"leader_api_base_url",
					"term",
				],
				"/api/admin/nodes": ["items"],
				"/api/admin/mesh/status": [
					"generated_at",
					"revision",
					"local",
					"peers",
					"events",
				],
			},
		});

		expect(result.kind).toBe("compatible");
		if (result.kind === "compatible") expect(result.profile.minor).toBe("3.22");
	});

	it("rejects an incomplete current-only fingerprint", () => {
		expect(
			resolveApiCompatibility({
				fingerprint: {
					"/api/health": ["status"],
					"/api/cluster/info": [
						"cluster_id",
						"node_id",
						"role",
						"leader_api_base_url",
						"term",
					],
					"/api/admin/nodes": ["items"],
					"/api/admin/mesh/status": ["revision"],
				},
			}).kind,
		).toBe("incompatible");
	});

	it("builds fallback fingerprint probes from the release inventories", async () => {
		const fetchMock = vi
			.fn()
			.mockResolvedValueOnce(new Response("not found", { status: 404 }))
			.mockResolvedValueOnce(new Response("not found", { status: 404 }))
			.mockResolvedValueOnce(
				new Response(JSON.stringify({ status: "ok" }), { status: 200 }),
			)
			.mockResolvedValueOnce(
				new Response(
					JSON.stringify({
						cluster_id: "fixture",
						node_id: "fixture",
						role: "leader",
						leader_api_base_url: "",
						term: 0,
					}),
					{ status: 200 },
				),
			)
			.mockResolvedValueOnce(
				new Response(JSON.stringify({ items: [] }), { status: 200 }),
			)
			.mockResolvedValueOnce(new Response("not found", { status: 404 }));
		vi.stubGlobal("fetch", fetchMock);

		const { fetchApiCompatibility } = await import("./apiCompatibility");
		const result = await fetchApiCompatibility({ adminToken: "fixture-token" });

		expect(result.kind).toBe("compatible");
		expect(fetchMock.mock.calls.slice(2).map(([path]) => path)).toEqual([
			"/api/health",
			"/api/cluster/info",
			"/api/admin/nodes",
			"/api/admin/mesh/status",
		]);
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

	it("requires an inventoried capability to select an additive profile", () => {
		expect(
			resolveApiCompatibility({
				releaseTag: "v3.23.0",
				capabilities: ["admin.endpoint-mihomo-smux"],
			}).kind,
		).toBe("incompatible");
	});

	it("does not downgrade a malformed capabilities response", async () => {
		const fetchMock = vi
			.fn()
			.mockResolvedValueOnce(
				new Response(JSON.stringify({ capabilities: 3 }), { status: 200 }),
			);
		vi.stubGlobal("fetch", fetchMock);

		const { fetchApiCompatibility } = await import("./apiCompatibility");
		const result = await fetchApiCompatibility();

		expect(result).toEqual({
			kind: "incompatible",
			reason: expect.stringContaining("invalid or failed"),
		});
		expect(fetchMock).toHaveBeenCalledTimes(1);
	});
});
