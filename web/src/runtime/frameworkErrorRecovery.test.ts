import { describe, expect, it, vi } from "vitest";

import { BackendApiError } from "../api/backendError";
import {
	classifyFrameworkError,
	createDiagnosticDetails,
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

	it("recovers with the active worker and activates a waiting replacement", async () => {
		const update = vi.fn(async () => undefined);
		const listeners = new Set<(event: MessageEvent) => void>();
		const controllerChangeListeners = new Set<() => void>();
		let waiting: { postMessage: (message: unknown) => void } | null = null;
		const waitingPostMessage = vi.fn(() => {
			waiting = null;
			for (const listener of controllerChangeListeners) listener();
		});
		const serviceWorkerContainer = {
			getRegistration: vi.fn(async () => ({ update, waiting })),
			controller: {
				postMessage: vi.fn((message: unknown) => {
					const requestId = (message as { requestId?: string }).requestId;
					for (const listener of listeners) {
						listener({
							data: {
								type: "XP_CACHE_RECOVERY_READY",
								buildId: "replacement-build-456",
								requestId,
								deleted: ["xp-app-shell-build-123"],
							},
						} as MessageEvent);
					}
				}),
			},
			addEventListener: vi.fn((type, listener) => {
				if (type === "message") listeners.add(listener);
				else controllerChangeListeners.add(listener);
			}),
			removeEventListener: vi.fn((type, listener) => {
				if (type === "message") listeners.delete(listener);
				else controllerChangeListeners.delete(listener);
			}),
		};

		await expect(
			recoverXpAppShell({
				isOnline: true,
				serviceWorkerContainer,
			}),
		).resolves.toEqual({
			status: "cleared",
			deleted: ["xp-app-shell-build-123"],
		});
		expect(update).toHaveBeenCalledOnce();

		waiting = { postMessage: waitingPostMessage };
		await expect(
			recoverXpAppShell({
				isOnline: true,
				serviceWorkerContainer,
			}),
		).resolves.toEqual({
			status: "cleared",
			deleted: ["xp-app-shell-build-123"],
		});
		expect(serviceWorkerContainer.controller.postMessage).toHaveBeenCalledWith(
			expect.objectContaining({
				type: "XP_REQUEST_CACHE_RECOVERY",
				buildId: "active",
			}),
		);
		expect(
			(
				serviceWorkerContainer.controller.postMessage.mock.calls[0][0] as {
					requestId?: string;
				}
			).requestId,
		).toEqual(expect.any(String));
		expect(waitingPostMessage).toHaveBeenCalledWith({ type: "SKIP_WAITING" });
	});
});
