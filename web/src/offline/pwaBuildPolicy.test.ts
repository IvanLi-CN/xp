import { describe, expect, it } from "vitest";

import {
	appShellCacheName,
	canDeleteBuildCache,
	isTransientBuildCacheName,
	selectBuildForRequest,
} from "./pwaBuildPolicy";

describe("pwa build policy", () => {
	it("uses the active build for navigations and the declared owner for assets", () => {
		expect(
			selectBuildForRequest({
				requestMode: "navigate",
				activeBuildId: "2026.08.05-new",
				clientBuildId: "2026.08.04-old",
			}),
		).toBe("2026.08.05-new");

		expect(
			selectBuildForRequest({
				requestMode: "asset",
				activeBuildId: "2026.08.05-new",
				clientBuildId: "2026.08.04-old",
			}),
		).toBe("2026.08.04-old");

		expect(
			selectBuildForRequest({
				requestMode: "asset",
				activeBuildId: "2026.08.05-new",
				clientBuildId: null,
			}),
		).toBeNull();
	});

	it("does not delete a build while a controlled client owns it", () => {
		expect(
			canDeleteBuildCache("2026.08.04-old", {
				"client-a": "2026.08.04-old",
			}),
		).toBe(false);
		expect(
			canDeleteBuildCache("2026.08.04-old", {
				"client-a": "2026.08.05-new",
			}),
		).toBe(true);
	});

	it("keeps staging and rollback caches out of build cleanup", () => {
		expect(isTransientBuildCacheName("xp-app-shell-install-123")).toBe(true);
		expect(isTransientBuildCacheName("xp-app-shell-recovery-123")).toBe(true);
		expect(isTransientBuildCacheName("xp-app-shell-backup-123")).toBe(true);
		expect(isTransientBuildCacheName("xp-app-shell-2026.08.05")).toBe(false);
	});

	it("names app-shell caches by build id", () => {
		expect(appShellCacheName("2026.08.05-new")).toBe(
			"xp-app-shell-2026.08.05-new",
		);
	});
});
