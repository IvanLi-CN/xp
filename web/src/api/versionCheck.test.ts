import { afterEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { VersionCheckResponseSchema, fetchVersionCheck } from "./versionCheck";

const comparableResponse = {
	current: { package: "0.1.0", release_tag: "v0.1.0" },
	latest: {
		release_tag: "v0.2.0",
		published_at: fixtureCatalog.slotString.s54(),
	},
	has_update: true,
	checked_at: fixtureCatalog.slotString.s54(),
	compare_reason: "semver",
	source: {
		kind: "github-releases",
		repo: "IvanLi-CN/xp",
		api_base: "https://api.github.com",
		channel: "stable",
	},
};

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("VersionCheckResponseSchema", () => {
	it("parses a comparable response", () => {
		const data = VersionCheckResponseSchema.parse(comparableResponse);

		expect(data.has_update).toBe(true);
		expect(data.latest.release_tag).toBe("v0.2.0");
	});

	it("parses an uncomparable response", () => {
		const data = VersionCheckResponseSchema.parse({
			current: { package: "0.1.0", release_tag: "v0.1.0" },
			latest: { release_tag: "main" },
			has_update: null,
			checked_at: fixtureCatalog.slotString.s54(),
			compare_reason: "uncomparable",
			source: {
				kind: "github-releases",
				repo: "IvanLi-CN/xp",
				api_base: "https://api.github.com",
				channel: "stable",
			},
		});

		expect(data.has_update).toBeNull();
	});
});

describe("fetchVersionCheck", () => {
	it("uses the cached endpoint by default", async () => {
		const fetchMock = vi.fn().mockResolvedValue(
			new Response(JSON.stringify(comparableResponse), {
				status: 200,
				headers: { "Content-Type": "application/json" },
			}),
		);
		vi.stubGlobal("fetch", fetchMock);

		await fetchVersionCheck();

		expect(fetchMock).toHaveBeenCalledWith(
			"/api/version/check",
			expect.objectContaining({ method: "GET" }),
		);
	});

	it("asks the backend to refresh when forced by the user", async () => {
		const fetchMock = vi.fn().mockResolvedValue(
			new Response(JSON.stringify(comparableResponse), {
				status: 200,
				headers: { "Content-Type": "application/json" },
			}),
		);
		vi.stubGlobal("fetch", fetchMock);

		await fetchVersionCheck({ refresh: true, adminToken: "admin-token" });

		expect(fetchMock).toHaveBeenCalledWith(
			"/api/version/check?refresh=1",
			expect.objectContaining({
				method: "GET",
				headers: expect.objectContaining({
					Authorization: "Bearer admin-token",
				}),
			}),
		);
	});
});
