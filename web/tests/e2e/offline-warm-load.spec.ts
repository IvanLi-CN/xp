import { expect, test } from "@playwright/test";

import { setAdminToken, setupApiMocks } from "./helpers";

test.skip(
	process.env.E2E_USE_PREVIEW !== "1",
	"offline warm-load requires a preview build with service worker support",
);

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
