import { describe, expect, it } from "vitest";

import { VersionCheckResponseSchema } from "./versionCheck";

describe("VersionCheckResponseSchema", () => {
	it("parses a comparable response", () => {
		const data = VersionCheckResponseSchema.parse({
			current: { package: "0.1.0", release_tag: "v0.1.0" },
			latest: { release_tag: "v0.2.0", published_at: "2026-01-31T00:00:00Z" },
			has_update: true,
			checked_at: "2026-01-31T00:00:00Z",
			compare_reason: "semver",
			source: {
				kind: "github-releases",
				repo: "IvanLi-CN/xp",
				api_base: "https://api.github.com",
				channel: "stable",
			},
		});

		expect(data.has_update).toBe(true);
		expect(data.latest.release_tag).toBe("v0.2.0");
	});

	it("parses an uncomparable response", () => {
		const data = VersionCheckResponseSchema.parse({
			current: { package: "0.1.0", release_tag: "v0.1.0" },
			latest: { release_tag: "main" },
			has_update: null,
			checked_at: "2026-01-31T00:00:00Z",
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
