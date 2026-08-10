import { expect, test } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

import { setAdminToken, setupApiMocks } from "./helpers";

test.skip(
	process.env.E2E_USE_PREVIEW !== "1",
	"offline warm-load requires a preview build with service worker support",
);

test("renders the static fallback when the entry module cannot load", async ({
	page,
}) => {
	await page.route(/\/assets\/index-[^/]+\.js/, (route) => route.abort());
	await page.goto("/");

	await expect(
		page.getByRole("heading", { name: "xp could not start" }),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "Reload app" })).toBeVisible();
	await page.getByText("Technical details").click();
	await expect(page.getByText("entry resource failed to load")).toBeVisible();
});

test("warm-loads cached nodes page offline after first successful visit", async ({
	context,
	page,
}) => {
	await setAdminToken(page);
	await setupApiMocks(page, { mockStatusEvents: false });

	await page.goto("/nodes");
	await expect(
		page.getByRole("heading", { name: "Nodes", exact: true }),
	).toBeVisible();
	await expect(page.getByText("1 node total")).toBeVisible();
	await page.waitForFunction(async () => {
		const db = await new Promise<IDBDatabase | null>((resolve, reject) => {
			const request = indexedDB.open("xp");
			request.onerror = () => reject(request.error);
			request.onupgradeneeded = () => resolve(null);
			request.onsuccess = () => resolve(request.result);
		});
		if (!db) return false;
		if (!db.objectStoreNames.contains("react_query_cache")) {
			db.close();
			return false;
		}
		const cachedState = await new Promise<unknown>((resolve, reject) => {
			const tx = db.transaction("react_query_cache", "readonly");
			const request = tx.objectStore("react_query_cache").get("xp-react-query");
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		db.close();
		const parsedState =
			typeof cachedState === "string"
				? (JSON.parse(cachedState) as {
						clientState?: {
							queries?: Array<{
								queryKey?: unknown[];
								state?: { status?: string };
							}>;
						};
					})
				: (cachedState as {
						clientState?: {
							queries?: Array<{
								queryKey?: unknown[];
								state?: { status?: string };
							}>;
						};
					} | null);
		const queries = parsedState?.clientState?.queries;
		if (!Array.isArray(queries)) return false;
		return queries.some(
			(query) =>
				Array.isArray(query.queryKey) &&
				query.queryKey[0] === "adminNodesRuntime" &&
				query.state?.status === "success",
		);
	});

	await page.reload();
	await page.waitForFunction(async () => {
		if (
			!("serviceWorker" in navigator) ||
			!navigator.serviceWorker.controller
		) {
			return false;
		}
		const registration = await navigator.serviceWorker.ready;
		if (!registration.active?.scriptURL.endsWith("/sw.js")) return false;
		const appShellCache = (await caches.keys()).find((name) =>
			name.startsWith("xp-app-shell-"),
		);
		if (!appShellCache) return false;
		const cache = await caches.open(appShellCache);
		const cachedUrls = new Set(
			(await cache.keys()).map((request) => request.url),
		);
		if (
			!["/index.html", "/site.webmanifest"].every((path) =>
				cachedUrls.has(new URL(path, location.origin).href),
			)
		) {
			return false;
		}
		if (
			![...cachedUrls].some((url) =>
				/\.(?:js|css)$/.test(new URL(url).pathname),
			)
		) {
			return false;
		}
		const db = await new Promise<IDBDatabase | null>((resolve, reject) => {
			const request = indexedDB.open("xp_sw_metadata");
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		if (!db?.objectStoreNames.contains("client_build_owners")) {
			db?.close();
			return false;
		}
		const owners = await new Promise<unknown[]>((resolve, reject) => {
			const request = db
				.transaction("client_build_owners", "readonly")
				.objectStore("client_build_owners")
				.getAll();
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		db.close();
		return owners.some(
			(owner) =>
				typeof owner === "object" &&
				owner !== null &&
				"clientId" in owner &&
				"buildId" in owner,
		);
	});
	await page.unroute("**/api/**");
	await page.waitForFunction(() => navigator.serviceWorker.controller !== null);
	await context.setOffline(true);

	const startedAt = Date.now();
	await page.reload({ waitUntil: "domcontentloaded" });
	await expect(
		page.getByRole("heading", { name: "Nodes", exact: true }),
	).toBeVisible({ timeout: 1000 });
	expect(Date.now() - startedAt).toBeLessThan(1000);

	await expect(page.getByText("Offline node inventory")).toBeVisible();
	await expect(page.getByText(/Last successful sync:/)).toBeVisible();
	await expect(page.getByText("1 node total")).toBeVisible();
	await expect(
		page.getByRole("button", { name: "Create token" }),
	).toBeDisabled();
});

test("keeps the active precache when a replacement install is interrupted", async ({
	context,
	page,
}) => {
	await setAdminToken(page);
	await setupApiMocks(page, { mockStatusEvents: false });
	await page.goto("/nodes");
	await expect(
		page.getByRole("heading", { name: "Nodes", exact: true }),
	).toBeVisible();
	await page.waitForFunction(async () => {
		const appShell = (await caches.keys()).find((name) =>
			name.startsWith("xp-app-shell-"),
		);
		if (!appShell) return false;
		const cache = await caches.open(appShell);
		const urls = (await cache.keys()).map((request) => request.url);
		return (
			urls.includes(new URL("/index.html", location.origin).href) &&
			urls.some((url) => /\.js$/.test(new URL(url).pathname)) &&
			urls.length >= 30
		);
	});

	const secondPage = await context.newPage();
	await setAdminToken(secondPage);
	await setupApiMocks(secondPage, { mockStatusEvents: false });
	await secondPage.goto("/nodes");
	await expect(
		secondPage.getByRole("heading", { name: "Nodes", exact: true }),
	).toBeVisible();
	await page.waitForFunction(async () => {
		const appShell = (await caches.keys()).find((name) =>
			name.startsWith("xp-app-shell-"),
		);
		if (!appShell) return false;
		return (
			(await caches.open(appShell).then((cache) => cache.keys())).length >= 30
		);
	});
	const before = await page.evaluate(async () => {
		const appShell = (await caches.keys()).find((name) =>
			name.startsWith("xp-app-shell-"),
		);
		if (!appShell) throw new Error("active app-shell cache missing");
		const cache = await caches.open(appShell);
		return {
			appShell,
			urls: (await cache.keys()).map((request) => request.url),
		};
	});
	await page.waitForFunction(async () => {
		const db = await new Promise<IDBDatabase>((resolve, reject) => {
			const request = indexedDB.open("xp_sw_metadata");
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		if (!db.objectStoreNames.contains("client_build_owners")) {
			db.close();
			return false;
		}
		const owners = await new Promise<unknown[]>((resolve, reject) => {
			const request = db
				.transaction("client_build_owners", "readonly")
				.objectStore("client_build_owners")
				.getAll();
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		db.close();
		return owners.length >= 2;
	});
	const owners = await page.evaluate(async () => {
		const db = await new Promise<IDBDatabase>((resolve, reject) => {
			const request = indexedDB.open("xp_sw_metadata");
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		const result = await new Promise<unknown[]>((resolve, reject) => {
			const request = db
				.transaction("client_build_owners", "readonly")
				.objectStore("client_build_owners")
				.getAll();
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		db.close();
		return result;
	});
	const ownedBuilds = owners
		.filter(
			(owner): owner is { buildId: string } =>
				typeof owner === "object" &&
				owner !== null &&
				typeof (owner as { buildId?: unknown }).buildId === "string",
		)
		.map((owner) => owner.buildId);
	expect(ownedBuilds.length).toBeGreaterThanOrEqual(2);
	expect(new Set(ownedBuilds).size).toBe(1);

	await page.route("**/sw.js", (route) => route.abort("failed"));
	await page.evaluate(async () => {
		const registration = await navigator.serviceWorker.ready;
		try {
			await registration.update();
		} catch {
			// An interrupted update is expected to leave the incumbent active.
		}
	});
	await page.waitForFunction(async () => {
		const registration = await navigator.serviceWorker.ready;
		return registration.waiting === null;
	});
	const after = await page.evaluate(async () => {
		const names = await caches.keys();
		const appShell = names.find((name) => name.startsWith("xp-app-shell-"));
		if (!appShell) return null;
		const cache = await caches.open(appShell);
		return (await cache.keys()).map((request) => request.url);
	});
	expect(after).toEqual(before.urls);
});

test("repairs an incomplete active precache through cache recovery", async ({
	page,
}) => {
	await setAdminToken(page);
	await setupApiMocks(page, { mockStatusEvents: false });
	await page.goto("/nodes");
	await expect(
		page.getByRole("heading", { name: "Nodes", exact: true }),
	).toBeVisible();
	await page.waitForFunction(() => Boolean(navigator.serviceWorker.controller));

	const build = await page.evaluate(async () => {
		const buildId = window.__XP_WEB_BUILD_ID__;
		const cacheName = (await caches.keys()).find((name) =>
			name.startsWith("xp-app-shell-"),
		);
		if (!cacheName) throw new Error("app-shell cache missing");
		const cache = await caches.open(cacheName);
		const targetRequest = (await cache.keys())[0];
		if (!targetRequest) throw new Error("app-shell cache is empty");
		await cache.delete(targetRequest);
		return { buildId, cacheName, targetUrl: targetRequest.url };
	});
	await expect(
		page.evaluate(async ({ cacheName, targetUrl }) => {
			const cache = await caches.open(cacheName);
			return (await cache.match(targetUrl)) === undefined;
		}, build),
	).resolves.toBe(true);

	const recoveryResult = await page.evaluate(
		({ buildId }) =>
			new Promise<string>((resolve, reject) => {
				const controller = navigator.serviceWorker.controller;
				if (!controller) {
					reject(new Error("service worker controller missing"));
					return;
				}
				const timeoutId = window.setTimeout(
					() => reject(new Error("cache recovery timed out")),
					10_000,
				);
				const onMessage = (event: MessageEvent) => {
					if (event.data?.buildId !== buildId) return;
					if (
						event.data?.type !== "XP_CACHE_RECOVERY_READY" &&
						event.data?.type !== "XP_CACHE_RECOVERY_UNAVAILABLE"
					) {
						return;
					}
					window.clearTimeout(timeoutId);
					navigator.serviceWorker.removeEventListener("message", onMessage);
					resolve(event.data.type);
				};
				navigator.serviceWorker.addEventListener("message", onMessage);
				controller.postMessage({
					type: "XP_REQUEST_CACHE_RECOVERY",
					buildId,
				});
			}),
		{ buildId: build.buildId },
	);
	expect(recoveryResult).toBe("XP_CACHE_RECOVERY_READY");

	await expect(
		page.evaluate(async ({ cacheName, targetUrl }) => {
			const cache = await caches.open(cacheName);
			return (await cache.match(targetUrl)) !== undefined;
		}, build),
	).resolves.toBe(true);
});

test("holds a complete waiting worker until the user confirms activation", async ({
	context,
	page,
}) => {
	await setAdminToken(page);
	await setupApiMocks(page, { mockStatusEvents: false });
	await page.goto("/nodes");
	await expect(
		page.getByRole("heading", { name: "Nodes", exact: true }),
	).toBeVisible();
	await page.reload();
	await page.waitForFunction(() => Boolean(navigator.serviceWorker.controller));
	const oldTab = await context.newPage();
	await setAdminToken(oldTab);
	await setupApiMocks(oldTab, { mockStatusEvents: false });
	await oldTab.goto("/nodes");
	await expect(
		oldTab.getByRole("heading", { name: "Nodes", exact: true }),
	).toBeVisible();
	await oldTab.waitForFunction(() =>
		Boolean(navigator.serviceWorker.controller),
	);
	const activeBuildId = await page.evaluate(
		() => window.__XP_WEB_BUILD_ID__ ?? "",
	);

	await page.evaluate(async () => {
		await navigator.serviceWorker.register(`/sw.js?e2e-waiting=${Date.now()}`, {
			scope: "/",
		});
	});
	const waitingState = await page.evaluate(async () => {
		const registration = await navigator.serviceWorker.getRegistration();
		if (!registration) throw new Error("service worker registration missing");
		const deadline = Date.now() + 5_000;
		while (Date.now() < deadline) {
			if (registration.waiting) {
				const waitingBuild = await caches.keys();
				return {
					waitingScript: registration.waiting.scriptURL,
					appShells: waitingBuild.filter((name) =>
						name.startsWith("xp-app-shell-"),
					),
				};
			}
			await new Promise((resolve) => setTimeout(resolve, 50));
		}
		throw new Error("waiting service worker did not appear");
	});
	expect(waitingState.waitingScript).toMatch(/\/sw\.js/);
	expect(waitingState.waitingScript).toContain("e2e-waiting=");
	expect(waitingState.appShells.length).toBeGreaterThan(0);
	expect(waitingState.appShells).toContain(`xp-app-shell-${activeBuildId}`);
	expect(
		waitingState.appShells.some(
			(cacheName) => cacheName !== `xp-app-shell-${activeBuildId}`,
		),
	).toBe(true);
	await expect(page.getByText("A newer web bundle is ready.")).toBeVisible();
	const lazyAssetStatuses: number[] = [];
	const oldDocumentBuildId = await oldTab.evaluate(
		() => window.__XP_WEB_BUILD_ID__ ?? "",
	);
	expect(oldDocumentBuildId).toBe(activeBuildId);
	oldTab.on("response", (response) => {
		if (/\/assets\/.*\.js(?:\?|$)/.test(response.url())) {
			lazyAssetStatuses.push(response.status());
		}
	});
	await oldTab.goto("/users");
	await expect(
		oldTab.getByRole("heading", { name: "Users", exact: true }),
	).toBeVisible();
	expect(await oldTab.evaluate(() => window.__XP_WEB_BUILD_ID__ ?? "")).toBe(
		oldDocumentBuildId,
	);
	expect(lazyAssetStatuses.length).toBeGreaterThan(0);
	expect(lazyAssetStatuses.every((status) => status === 200)).toBe(true);
	await page.getByRole("button", { name: "Reload", exact: true }).click();
	await page.waitForFunction(async () => {
		const registration = await navigator.serviceWorker.getRegistration();
		return !registration?.waiting;
	});
});

test("migrates a legacy client around a complete XP waiting worker", async ({
	context,
	page,
}) => {
	const token = `${Date.now()}`;
	const tokenStorageKey = fixtureCatalog.token.fixture79();
	const tokenStorageValue = fixtureCatalog.token.fixture80();
	await page.goto("/__e2e_legacy_client__.html");
	const legacyCacheName = await page.evaluate(
		() => `workbox-precache-v2-${location.origin}/`,
	);
	await page.evaluate(async () => {
		await navigator.serviceWorker.register("/__e2e_legacy_worker__.js", {
			scope: "/",
		});
	});
	await page.waitForFunction(() =>
		navigator.serviceWorker.controller?.scriptURL.includes(
			"/__e2e_legacy_worker__.js",
		),
	);
	await page.evaluate(
		async ({ legacyCache, tokenKey, tokenValue }) => {
			await caches.open(legacyCache);
			localStorage.setItem(tokenKey, tokenValue);
			localStorage.setItem("xp_ui_density", "compact");
			await new Promise<void>((resolve, reject) => {
				const request = indexedDB.open("xp", 1);
				request.onupgradeneeded = () => {
					if (!request.result.objectStoreNames.contains("react_query_cache"))
						request.result.createObjectStore("react_query_cache");
				};
				request.onerror = () => reject(request.error);
				request.onsuccess = () => {
					const db = request.result;
					const transaction = db.transaction("react_query_cache", "readwrite");
					transaction
						.objectStore("react_query_cache")
						.put("preserved-query-cache", "legacy-sentinel");
					transaction.onerror = () => reject(transaction.error);
					transaction.oncomplete = () => {
						db.close();
						resolve();
					};
				};
			});
			let controllerChanges = 0;
			navigator.serviceWorker.addEventListener("controllerchange", () => {
				controllerChanges += 1;
			});
			Object.defineProperty(window, "__xp_e2e_controller_changes", {
				get: () => controllerChanges,
				configurable: true,
			});
		},
		{
			legacyCache: legacyCacheName,
			tokenKey: tokenStorageKey,
			tokenValue: tokenStorageValue,
		},
	);

	const predecessorBuildId = `e2e-legacy-predecessor-${token}`;
	await page.evaluate(async (workerToken) => {
		await navigator.serviceWorker.register(
			`/sw.js?e2e-legacy-predecessor=${workerToken}`,
			{ scope: "/" },
		);
	}, token);
	const predecessorReady = await page.evaluate(
		async ({ workerToken, buildId }) => {
			const deadline = Date.now() + 10_000;
			while (Date.now() < deadline) {
				const registration = await navigator.serviceWorker.getRegistration();
				if (
					registration &&
					!registration.installing &&
					registration.waiting?.scriptURL.includes(
						`e2e-legacy-predecessor=${workerToken}`,
					) &&
					(await caches.has(`xp-app-shell-${buildId}`))
				) {
					return true;
				}
				await new Promise((resolve) => setTimeout(resolve, 50));
			}
			return false;
		},
		{ workerToken: token, buildId: predecessorBuildId },
	);
	expect(predecessorReady).toBe(true);
	const predecessorState = await page.evaluate(async (buildId) => {
		const cache = await caches.open(`xp-app-shell-${buildId}`);
		const metadataResponse = await cache.match(
			new URL("/__xp_build_metadata__", location.origin).href,
		);
		const metadata = (await metadataResponse?.json()) as {
			buildId?: string;
			entries?: Array<{ url: string }>;
		} | null;
		const entries = metadata?.entries ?? [];
		return {
			metadataBuildId: metadata?.buildId ?? null,
			entryCount: entries.length,
			complete: await Promise.all(
				entries.map((entry) => cache.match(entry.url)),
			).then((responses) => responses.every(Boolean)),
		};
	}, predecessorBuildId);
	expect(predecessorState.metadataBuildId).toBe(predecessorBuildId);
	expect(predecessorState.entryCount).toBeGreaterThan(0);
	expect(predecessorState.complete).toBe(true);

	await page.evaluate(async (workerToken) => {
		await navigator.serviceWorker.register(
			`/sw.js?e2e-legacy-migration=${workerToken}`,
			{ scope: "/" },
		);
	}, token);
	await page.waitForFunction(
		(workerToken) =>
			navigator.serviceWorker
				.getRegistration()
				.then(
					(registration) =>
						registration?.active?.scriptURL.includes(
							`e2e-legacy-migration=${workerToken}`,
						) && !registration.waiting,
				),
		token,
	);
	const legacyTabState = await page.evaluate(
		async ({ legacyCache, predecessorCache }) => ({
			controller: navigator.serviceWorker.controller?.scriptURL ?? null,
			controllerChanges: (
				window as Window & { __xp_e2e_controller_changes?: number }
			).__xp_e2e_controller_changes,
			legacyCachePresent: await caches.has(legacyCache),
			predecessorCachePresent: await caches.has(predecessorCache),
		}),
		{
			legacyCache: legacyCacheName,
			predecessorCache: `xp-app-shell-${predecessorBuildId}`,
		},
	);
	expect(legacyTabState.controller).toContain("/__e2e_legacy_worker__.js");
	expect(legacyTabState.controllerChanges).toBe(0);
	expect(legacyTabState.legacyCachePresent).toBe(true);
	expect(legacyTabState.predecessorCachePresent).toBe(true);

	await page.close();
	const migratedPage = await context.newPage();
	await migratedPage.addInitScript(() => {
		sessionStorage.setItem(
			"xp.upgrade-observation",
			JSON.stringify({
				targetTag: "v3.23.0",
				deadlineAtMs: Date.now() + 30_000,
				startedAtMs: Date.now(),
				phase: "observing",
			}),
		);
	});
	await setAdminToken(migratedPage, fixtureCatalog.token.fixture80());
	await setupApiMocks(migratedPage, { mockStatusEvents: false });
	const runtimeErrors: string[] = [];
	migratedPage.on("console", (message) => {
		if (message.type() === "error") runtimeErrors.push(message.text());
	});
	migratedPage.on("pageerror", (error) => runtimeErrors.push(error.message));
	await migratedPage.goto("/nodes");
	await expect(
		migratedPage.getByRole("heading", { name: "Nodes", exact: true }),
	).toBeVisible();
	await expect(
		migratedPage.getByText(
			"Waiting for the node to reconnect and report the upgrade status.",
		),
	).toBeVisible();
	await migratedPage.waitForFunction(
		async ({ legacyCache, predecessorCache, workerToken }) => {
			const registration = await navigator.serviceWorker.getRegistration();
			return (
				registration?.active?.scriptURL.includes(
					`e2e-legacy-migration=${workerToken}`,
				) &&
				!(await caches.has(legacyCache)) &&
				!(await caches.has(predecessorCache))
			);
		},
		{
			legacyCache: legacyCacheName,
			predecessorCache: `xp-app-shell-${predecessorBuildId}`,
			workerToken: token,
		},
	);

	const preservedState = await migratedPage.evaluate(async (tokenKey) => {
		const db = await new Promise<IDBDatabase>((resolve, reject) => {
			const request = indexedDB.open("xp");
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		const queryCache = await new Promise<unknown>((resolve, reject) => {
			const request = db
				.transaction("react_query_cache", "readonly")
				.objectStore("react_query_cache")
				.get("legacy-sentinel");
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		db.close();
		return {
			storedToken: localStorage.getItem(tokenKey),
			density: localStorage.getItem("xp_ui_density"),
			queryCache,
		};
	}, tokenStorageKey);
	expect(preservedState).toEqual({
		storedToken: tokenStorageValue,
		density: "compact",
		queryCache: "preserved-query-cache",
	});
	expect(
		runtimeErrors.some((message) =>
			/Minified React error #185|Maximum update depth exceeded/i.test(message),
		),
	).toBe(false);
});

test("keeps the legacy worker and migration state unchanged when install fails", async ({
	page,
}) => {
	const token = `${Date.now()}`;
	await page.goto("/__e2e_legacy_client__.html");
	const legacyCacheName = await page.evaluate(
		() => `workbox-precache-v2-${location.origin}/`,
	);
	await page.evaluate(async (legacyCache) => {
		await navigator.serviceWorker.register("/__e2e_legacy_worker__.js", {
			scope: "/",
		});
		await caches.open(legacyCache);
	}, legacyCacheName);
	await page.waitForFunction(() =>
		navigator.serviceWorker.controller?.scriptURL.includes(
			"/__e2e_legacy_worker__.js",
		),
	);
	await page.evaluate(async (workerToken) => {
		try {
			await navigator.serviceWorker.register(
				`/sw.js?e2e-legacy-install-failure=${workerToken}`,
				{ scope: "/" },
			);
		} catch {
			// Browser registration may reject immediately or only after install.
		}
	}, token);
	await page.waitForFunction(async (legacyCache) => {
		const registration = await navigator.serviceWorker.getRegistration();
		return (
			registration?.active?.scriptURL.includes("/__e2e_legacy_worker__.js") &&
			!registration.installing &&
			!registration.waiting &&
			(await caches.has(legacyCache))
		);
	}, legacyCacheName);
	const migrationState = await page.evaluate(async () => {
		const databases = await indexedDB.databases();
		if (!databases.some((database) => database.name === "xp_sw_metadata"))
			return null;
		const db = await new Promise<IDBDatabase>((resolve, reject) => {
			const request = indexedDB.open("xp_sw_metadata");
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result);
		});
		if (!db.objectStoreNames.contains("migration_state")) {
			db.close();
			return null;
		}
		const state = await new Promise<unknown>((resolve, reject) => {
			const request = db
				.transaction("migration_state", "readonly")
				.objectStore("migration_state")
				.get("legacy-workbox");
			request.onerror = () => reject(request.error);
			request.onsuccess = () => resolve(request.result ?? null);
		});
		db.close();
		return state;
	});
	expect(migrationState).toBeNull();
});
