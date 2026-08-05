/// <reference lib="webworker" />

import {
	APP_SHELL_CACHE_PREFIX,
	appShellCacheName,
	buildIdFromCacheName,
	canDeleteBuildCache,
	selectBuildForRequest,
} from "./offline/pwaBuildPolicy";

declare const self: ServiceWorkerGlobalScope & {
	__WB_MANIFEST: Array<{ url: string; revision?: string }>;
};

type OwnerRecord = {
	clientId: string;
	buildId: string;
	updatedAt: number;
};

type ServiceWorkerMessage =
	| { type: "SKIP_WAITING" }
	| { type: "XP_DECLARE_BUILD"; buildId: string }
	| { type: "XP_RELEASE_BUILD" }
	| { type: "XP_REQUEST_CACHE_RECOVERY"; buildId: string };

const BUILD_ID = __XP_WEB_BUILD_ID__;
const CACHE_NAME = appShellCacheName(BUILD_ID);
const METADATA_DB_NAME = "xp_sw_metadata";
const METADATA_STORE_NAME = "client_build_owners";
const manifestEntries = self.__WB_MANIFEST;
const manifestUrls = new Set(
	manifestEntries.map(
		(entry) => new URL(entry.url, self.registration.scope).href,
	),
);

function isSameOriginStaticRequest(request: Request): boolean {
	const url = new URL(request.url);
	return (
		request.method === "GET" &&
		url.origin === self.location.origin &&
		!url.pathname.startsWith("/api/") &&
		!url.pathname.startsWith("/events")
	);
}

function openMetadataDb(): Promise<IDBDatabase> {
	return new Promise((resolve, reject) => {
		const request = indexedDB.open(METADATA_DB_NAME, 1);
		request.onupgradeneeded = () => {
			request.result.createObjectStore(METADATA_STORE_NAME, {
				keyPath: "clientId",
			});
		};
		request.onsuccess = () => resolve(request.result);
		request.onerror = () => reject(request.error);
	});
}

async function readOwners(): Promise<Record<string, string>> {
	const db = await openMetadataDb();
	return new Promise((resolve, reject) => {
		const transaction = db.transaction(METADATA_STORE_NAME, "readonly");
		const request = transaction.objectStore(METADATA_STORE_NAME).getAll();
		request.onsuccess = () => {
			const owners: Record<string, string> = {};
			for (const record of request.result as OwnerRecord[]) {
				if (record.clientId && record.buildId)
					owners[record.clientId] = record.buildId;
			}
			resolve(owners);
		};
		request.onerror = () => reject(request.error);
		transaction.oncomplete = () => db.close();
	});
}

async function writeOwner(clientId: string, buildId: string): Promise<void> {
	const db = await openMetadataDb();
	return new Promise((resolve, reject) => {
		const transaction = db.transaction(METADATA_STORE_NAME, "readwrite");
		transaction.objectStore(METADATA_STORE_NAME).put({
			clientId,
			buildId,
			updatedAt: Date.now(),
		} satisfies OwnerRecord);
		transaction.oncomplete = () => {
			db.close();
			resolve();
		};
		transaction.onerror = () => reject(transaction.error);
	});
}

async function deleteOwner(clientId: string): Promise<void> {
	const db = await openMetadataDb();
	return new Promise((resolve, reject) => {
		const transaction = db.transaction(METADATA_STORE_NAME, "readwrite");
		transaction.objectStore(METADATA_STORE_NAME).delete(clientId);
		transaction.oncomplete = () => {
			db.close();
			resolve();
		};
		transaction.onerror = () => reject(transaction.error);
	});
}

async function cleanupUnownedBuildCaches(
	owners: Record<string, string>,
): Promise<string[]> {
	const cacheNames = await caches.keys();
	const deleted: string[] = [];
	for (const cacheName of cacheNames) {
		if (!cacheName.startsWith(APP_SHELL_CACHE_PREFIX)) continue;
		const buildId = buildIdFromCacheName(cacheName);
		if (
			!buildId ||
			buildId === BUILD_ID ||
			!canDeleteBuildCache(buildId, owners)
		) {
			continue;
		}
		if (await caches.delete(cacheName)) deleted.push(cacheName);
	}
	return deleted;
}

async function reconcileOwnership(): Promise<string[]> {
	const windows = await self.clients.matchAll({
		type: "window",
		includeUncontrolled: true,
	});
	const owners = await readOwners();
	const liveClientIds = new Set(windows.map((client) => client.id));
	let hasUndeclaredClient = false;

	for (const client of windows) {
		if (owners[client.id]) continue;
		hasUndeclaredClient = true;
		client.postMessage({
			type: "XP_REQUEST_BUILD_DECLARATION",
			buildId: BUILD_ID,
		});
	}

	await Promise.all(
		Object.keys(owners)
			.filter((clientId) => !liveClientIds.has(clientId))
			.map(async (clientId) => {
				await deleteOwner(clientId);
				delete owners[clientId];
			}),
	);

	if (!hasUndeclaredClient) {
		return cleanupUnownedBuildCaches(owners);
	}
	return [];
}

async function completeBuildCache(): Promise<boolean> {
	const target = await caches.open(CACHE_NAME);
	const missing = [] as string[];
	for (const url of manifestUrls) {
		if (!(await target.match(url))) missing.push(url);
	}
	if (missing.length === 0) return true;

	const recoveryName = `${CACHE_NAME}-recovery-${Date.now()}`;
	const recovery = await caches.open(recoveryName);
	try {
		for (const url of manifestUrls) {
			const response = await fetch(new Request(url, { cache: "reload" }));
			if (!response.ok)
				throw new Error(`precache failed: ${url} (${response.status})`);
			await recovery.put(url, response);
		}
		for (const url of manifestUrls) {
			const response = await recovery.match(url);
			if (!response) throw new Error(`precache verification failed: ${url}`);
			await target.put(url, response);
		}
		await caches.delete(recoveryName);
		return true;
	} catch {
		await caches.delete(recoveryName);
		return false;
	}
}

async function installBuild(): Promise<void> {
	const stagingName = `${CACHE_NAME}-install-${Date.now()}`;
	const staging = await caches.open(stagingName);
	try {
		for (const url of manifestUrls) {
			const response = await fetch(new Request(url, { cache: "reload" }));
			if (!response.ok)
				throw new Error(`precache failed: ${url} (${response.status})`);
			await staging.put(url, response);
		}
		for (const url of manifestUrls) {
			if (!(await staging.match(url)))
				throw new Error(`precache verification failed: ${url}`);
		}
		await caches.delete(CACHE_NAME);
		const cache = await caches.open(CACHE_NAME);
		for (const url of manifestUrls) {
			const response = await staging.match(url);
			if (!response) throw new Error(`precache copy failed: ${url}`);
			await cache.put(url, response);
		}
		for (const url of manifestUrls) {
			if (!(await cache.match(url)))
				throw new Error(`precache verification failed: ${url}`);
		}
		await caches.delete(stagingName);
	} catch (error) {
		await caches.delete(stagingName);
		await caches.delete(CACHE_NAME);
		throw error;
	}
}

async function respondToClient(
	clientId: string | null,
	message: Record<string, unknown>,
): Promise<void> {
	if (!clientId) return;
	const client = await self.clients.get(clientId);
	client?.postMessage(message);
}

async function handleStaticRequest(event: FetchEvent): Promise<Response> {
	const requestUrl = new URL(event.request.url);
	const requestKind = event.request.mode === "navigate" ? "navigate" : "asset";
	if (requestKind === "asset" && !manifestUrls.has(requestUrl.href)) {
		return new Response("Static asset is not part of the active precache.", {
			status: 504,
			headers: { "Content-Type": "text/plain" },
		});
	}

	const owners = await readOwners();
	const buildId = selectBuildForRequest({
		requestMode: requestKind,
		activeBuildId: BUILD_ID,
		clientBuildId: event.clientId ? owners[event.clientId] : null,
	});
	const cache = await caches.open(appShellCacheName(buildId));
	const cacheUrl =
		requestKind === "navigate"
			? new URL("/index.html", self.registration.scope).href
			: requestUrl.href;
	const response = await cache.match(cacheUrl, { ignoreSearch: true });
	if (response) return response;

	await respondToClient(event.clientId || null, {
		type: "XP_CACHE_MISS",
		buildId,
		url: requestUrl.pathname,
	});
	return new Response("The selected application build is incomplete.", {
		status: 504,
		headers: { "Content-Type": "text/plain" },
	});
}

self.addEventListener("install", (event) => {
	event.waitUntil(installBuild());
});

self.addEventListener("activate", (event) => {
	event.waitUntil(
		(async () => {
			await self.clients.claim();
			await reconcileOwnership();
		})(),
	);
});

self.addEventListener("fetch", (event) => {
	if (!isSameOriginStaticRequest(event.request)) return;
	event.respondWith(handleStaticRequest(event));
});

self.addEventListener("message", (event: ExtendableMessageEvent) => {
	const message = event.data as ServiceWorkerMessage | undefined;
	const clientId =
		event.source && "id" in event.source ? event.source.id : null;
	event.waitUntil(
		(async () => {
			if (message?.type === "SKIP_WAITING") {
				self.skipWaiting();
				return;
			}
			if (message?.type === "XP_DECLARE_BUILD" && clientId) {
				if (!(await caches.has(appShellCacheName(message.buildId)))) {
					await respondToClient(clientId, {
						type: "XP_CACHE_MISS",
						buildId: message.buildId,
					});
					return;
				}
				await writeOwner(clientId, message.buildId);
				await reconcileOwnership();
				return;
			}
			if (message?.type === "XP_RELEASE_BUILD" && clientId) {
				await deleteOwner(clientId);
				await reconcileOwnership();
				return;
			}
			if (message?.type === "XP_REQUEST_CACHE_RECOVERY") {
				const complete =
					message.buildId === BUILD_ID && (await completeBuildCache());
				const deleted = complete ? await reconcileOwnership() : [];
				await respondToClient(clientId, {
					type: complete
						? "XP_CACHE_RECOVERY_READY"
						: "XP_CACHE_RECOVERY_UNAVAILABLE",
					buildId: message.buildId,
					deleted,
				});
			}
		})(),
	);
});
