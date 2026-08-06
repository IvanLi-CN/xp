import { describe, expect, it } from "vitest";

import {
	appShellCacheName,
	canDeleteBuildCache,
	isTransientBuildCacheName,
	legacyWorkboxPrecacheCacheName,
	selectBuildForRequest,
	selectWorkerInstallMode,
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

	it("never selects a process-wide pending build for an undeclared asset", () => {
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

	it("only bypasses waiting for an undeclared legacy Workbox client", () => {
		const legacyCacheName = legacyWorkboxPrecacheCacheName(
			"https://xp.example/",
		);
		expect(
			selectWorkerInstallMode({
				cacheNames: [
					legacyCacheName,
					appShellCacheName("intermediate-xp-build"),
				],
				currentCacheName: appShellCacheName("current-build"),
				legacyCacheName,
				liveClientCount: 1,
				validXpOwnerCount: 0,
			}),
		).toBe("legacy_migration");

		expect(
			selectWorkerInstallMode({
				cacheNames: [legacyCacheName, appShellCacheName("previous-xp-build")],
				currentCacheName: appShellCacheName("current-build"),
				legacyCacheName,
				liveClientCount: 2,
				validXpOwnerCount: 1,
			}),
		).toBe("normal_update");
	});

	it("keeps a regular XP-to-XP update waiting", () => {
		expect(
			selectWorkerInstallMode({
				cacheNames: [appShellCacheName("previous-xp-build")],
				currentCacheName: appShellCacheName("current-build"),
				legacyCacheName: legacyWorkboxPrecacheCacheName("https://xp.example/"),
				liveClientCount: 1,
				validXpOwnerCount: 1,
			}),
		).toBe("normal_update");
	});
});
