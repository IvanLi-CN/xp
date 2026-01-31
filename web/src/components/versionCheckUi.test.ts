import { describe, expect, it } from "vitest";

import {
	reduceVersionCheckUiState,
	shouldAutoCheckVersion,
	toSemverishReleaseTag,
} from "./versionCheckUi";

describe("toSemverishReleaseTag", () => {
	it("normalizes plain semver", () => {
		expect(toSemverishReleaseTag("0.1.2")).toBe("v0.1.2");
	});

	it("normalizes v-prefixed semver", () => {
		expect(toSemverishReleaseTag("v0.1.2")).toBe("v0.1.2");
	});

	it("rejects non-semverish strings", () => {
		expect(toSemverishReleaseTag("main")).toBeNull();
	});
});

describe("shouldAutoCheckVersion", () => {
	it("checks when never checked before", () => {
		expect(shouldAutoCheckVersion({ nowMs: 1000, lastAtMs: null })).toBe(true);
	});

	it("checks when cooldown elapsed", () => {
		expect(
			shouldAutoCheckVersion({
				nowMs: 60 * 60 * 1000 + 1,
				lastAtMs: 0,
				cooldownMs: 60 * 60 * 1000,
			}),
		).toBe(true);
	});

	it("skips when within cooldown", () => {
		expect(
			shouldAutoCheckVersion({
				nowMs: 10,
				lastAtMs: 0,
				cooldownMs: 1000,
			}),
		).toBe(false);
	});
});

describe("reduceVersionCheckUiState", () => {
	it("maps has_update=true to update_available", () => {
		const next = reduceVersionCheckUiState(
			{ kind: "idle" },
			{
				type: "success",
				data: {
					current: { package: "0.1.0", release_tag: "v0.1.0" },
					latest: { release_tag: "v0.2.0" },
					has_update: true,
					checked_at: "2026-01-31T00:00:00Z",
					compare_reason: "semver",
					source: {
						kind: "github-releases",
						repo: "acme/xp",
						api_base: "http://example.invalid",
						channel: "stable",
					},
				},
			},
		);
		expect(next.kind).toBe("update_available");
		if (next.kind !== "update_available") throw new Error("unexpected");
		expect(next.latest_tag).toBe("v0.2.0");
		expect(next.repo).toBe("acme/xp");
	});
});
