import { describe, expect, it, vi } from "vitest";

import { BackendApiError } from "../api/backendError";
import {
	classifyFrameworkError,
	clearXpAppShellCaches,
	createDiagnosticDetails,
	isXpAppShellCacheName,
	recoverXpAppShell,
} from "./frameworkErrorRecovery";

describe("framework error recovery", () => {
	it.each([
		[
			"chunk load failures",
			new Error("Failed to fetch dynamically imported module: /assets/app.js"),
			"chunk-load",
		],
		[
			"cache version mismatches",
			Object.assign(new Error("asset cache version mismatch"), {
				code: "CACHE_VERSION_MISMATCH",
			}),
			"cache-mismatch",
		],
		["offline failures", new TypeError("Failed to fetch"), "offline"],
		[
			"API incompatibility failures",
			new BackendApiError({
				status: 409,
				code: "api_incompatible",
				message: "This client is outside the API compatibility window.",
			}),
			"api-incompatibility",
		],
		[
			"React runtime failures",
			new Error("Minified React error #185"),
			"react-runtime",
		],
	] as const)("classifies %s", (_label, error, expected) => {
		expect(
			classifyFrameworkError(error, {
				isOnline: expected !== "offline",
			}),
		).toBe(expected);
	});

	it("classifies unrecognized errors as unknown", () => {
		expect(classifyFrameworkError(new Error("unexpected"))).toBe("unknown");
	});

	it("redacts credentials and sensitive query values from diagnostics", () => {
		const details = createDiagnosticDetails({
			error: new Error(
				"GET https://xp.example/api?token=secret-token&node=tokyo\n" +
					"Authorization: Bearer bearer-secret\n" +
					"jwt=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.signature-value",
			),
			category: "unknown",
			buildId: "3.22.5-abc123",
			path: "/nodes?login_token=login-secret",
			isOnline: true,
		});

		expect(details).toContain("category: unknown");
		expect(details).toContain("path: /nodes");
		expect(details).toContain("[REDACTED]");
		expect(details).not.toContain("secret-token");
		expect(details).not.toContain("bearer-secret");
		expect(details).not.toContain("login-secret");
		expect(details).not.toContain("eyJhbGciOiJIUzI1NiJ9");
	});

	it("reports the browser online state when no override is supplied", () => {
		Object.defineProperty(navigator, "onLine", {
			configurable: true,
			value: false,
		});

		expect(
			createDiagnosticDetails({
				error: new Error("Failed to fetch"),
				category: "offline",
			}),
		).toContain("online: false");
	});

	it("matches only XP app-shell cache names", () => {
		expect(
			isXpAppShellCacheName(
				"workbox-precache-v2-https://xp.example",
				"https://xp.example",
			),
		).toBe(true);
		expect(isXpAppShellCacheName("xp-app-shell-build-123")).toBe(true);
		expect(
			isXpAppShellCacheName(
				"workbox-runtime-https://xp.example",
				"https://xp.example",
			),
		).toBe(false);
		expect(isXpAppShellCacheName("react_query_cache")).toBe(false);
	});

	it("deletes only app-shell caches after a replacement is ready", async () => {
		const deleted: string[] = [];
		const cacheStorage = {
			keys: vi.fn(async () => [
				"workbox-precache-v2-https://xp.example",
				"xp-app-shell-build-123",
				"workbox-runtime-https://xp.example",
				"react_query_cache",
			]),
			delete: vi.fn(async (name: string) => {
				deleted.push(name);
				return true;
			}),
		};

		await expect(
			clearXpAppShellCaches({
				cacheStorage,
				origin: "https://xp.example",
				isOnline: true,
				replacementReady: async () => true,
			}),
		).resolves.toEqual({
			status: "cleared",
			deleted: [
				"workbox-precache-v2-https://xp.example",
				"xp-app-shell-build-123",
			],
		});
		expect(deleted).toEqual([
			"workbox-precache-v2-https://xp.example",
			"xp-app-shell-build-123",
		]);
	});

	it("does not touch caches when offline or when replacement is unavailable", async () => {
		const cacheStorage = {
			keys: vi.fn(async () => ["xp-app-shell-build-123"]),
			delete: vi.fn(async () => true),
		};

		await expect(
			clearXpAppShellCaches({
				cacheStorage,
				isOnline: false,
				replacementReady: async () => true,
			}),
		).resolves.toEqual({
			status: "skipped",
			reason: "offline",
			deleted: [],
		});

		await expect(
			clearXpAppShellCaches({
				cacheStorage,
				isOnline: true,
				replacementReady: async () => false,
			}),
		).resolves.toEqual({
			status: "skipped",
			reason: "replacement-unavailable",
			deleted: [],
		});
		expect(cacheStorage.keys).not.toHaveBeenCalled();
		expect(cacheStorage.delete).not.toHaveBeenCalled();
	});

	it("does not clear caches until a waiting worker proves a complete replacement", async () => {
		const cacheStorage = {
			keys: vi.fn(async () => ["xp-app-shell-build-123"]),
			delete: vi.fn(async () => true),
		};
		const update = vi.fn(async () => undefined);

		await expect(
			recoverXpAppShell({
				cacheStorage,
				isOnline: true,
				serviceWorkerContainer: {
					getRegistration: vi.fn(async () => ({
						update,
						waiting: null,
					})),
				},
			}),
		).resolves.toEqual({
			status: "skipped",
			reason: "replacement-unavailable",
			deleted: [],
		});
		expect(update).toHaveBeenCalledOnce();
		expect(cacheStorage.keys).not.toHaveBeenCalled();

		await expect(
			recoverXpAppShell({
				cacheStorage,
				isOnline: true,
				serviceWorkerContainer: {
					getRegistration: vi.fn(async () => ({
						update,
						waiting: {},
					})),
				},
			}),
		).resolves.toEqual({
			status: "cleared",
			deleted: ["xp-app-shell-build-123"],
		});
	});
});
