/// <reference lib="webworker" />

import {
	APP_SHELL_CACHE_PREFIX,
	appShellCacheName,
	buildIdFromCacheName,
	canDeleteBuildCache,
	isTransientBuildCacheName,
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
	| { type: "XP_REQUEST_CACHE_RECOVERY"; buildId: string | "active" };

type BuildMetadata = {
	buildId: string;
	entries: Array<{ url: string; revision: string | null }>;
};

const BUILD_ID = __XP_WEB_BUILD_ID__;
const CACHE_NAME = appShellCacheName(BUILD_ID);
let shouldClaimClients = false;
const METADATA_DB_NAME = "xp_sw_metadata";
const METADATA_STORE_NAME = "client_build_owners";
const RETIRED_BUILD_STORE_NAME = "retired_builds";
const manifestEntries = self.__WB_MANIFEST;
const normalizedManifestEntries = manifestEntries.map((entry) => ({
	url: new URL(entry.url, self.registration.scope).href,
	revision: entry.revision ?? null,
}));
const manifestUrls = new Set(
	normalizedManifestEntries.map((entry) => entry.url),
);
const BUILD_METADATA_URL = new URL(
	"/__xp_build_metadata__",
	self.registration.scope,
).href;
const pendingNavigationBuilds = new Map<string, string>();
let cacheMutation: Promise<unknown> = Promise.resolve();

function isSameOriginStaticRequest(request: Request): boolean {
	const url = new URL(request.url);
	return (
		request.method === "GET" &&
		url.origin === self.location.origin &&
		url.pathname !== "/sw.js" &&
		!url.pathname.startsWith("/api/") &&
		!url.pathname.startsWith("/events")
	);
}

function openMetadataDb(): Promise<IDBDatabase> {
	return new Promise((resolve, reject) => {
		const request = indexedDB.open(METADATA_DB_NAME, 2);
		request.onupgradeneeded = () => {
			const db = request.result;
			if (!db.objectStoreNames.contains(METADATA_STORE_NAME)) {
				db.createObjectStore(METADATA_STORE_NAME, { keyPath: "clientId" });
			}
			if (!db.objectStoreNames.contains(RETIRED_BUILD_STORE_NAME)) {
				db.createObjectStore(RETIRED_BUILD_STORE_NAME, { keyPath: "buildId" });
			}
		};
		request.onsuccess = () => resolve(request.result);
		request.onerror = () => reject(request.error);
	});
}

async function readRetiredBuilds(): Promise<Set<string>> {
	const db = await openMetadataDb();
	return new Promise((resolve, reject) => {
		const transaction = db.transaction(RETIRED_BUILD_STORE_NAME, "readonly");
		const request = transaction.objectStore(RETIRED_BUILD_STORE_NAME).getAll();
		request.onsuccess = () => {
			resolve(
				new Set(
					(request.result as Array<{ buildId?: unknown }>)
						.filter((record) => typeof record.buildId === "string")
						.map((record) => record.buildId as string),
				),
			);
		};
		request.onerror = () => reject(request.error);
		transaction.oncomplete = () => db.close();
	});
}

async function setRetiredBuild(
	buildId: string,
	retired: boolean,
): Promise<void> {
	const db = await openMetadataDb();
	return new Promise((resolve, reject) => {
		const transaction = db.transaction(RETIRED_BUILD_STORE_NAME, "readwrite");
		const store = transaction.objectStore(RETIRED_BUILD_STORE_NAME);
		if (retired) store.put({ buildId, retiredAt: Date.now() });
		else store.delete(buildId);
		transaction.oncomplete = () => {
			db.close();
			resolve();
		};
		transaction.onerror = () => reject(transaction.error);
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

async function retireBuildIfUnowned(
	buildId: string | undefined,
	owners: Record<string, string>,
): Promise<void> {
	if (buildId && !Object.values(owners).includes(buildId)) {
		await setRetiredBuild(buildId, true);
	}
}

async function cleanupUnownedBuildCaches(
	owners: Record<string, string>,
	retiredBuilds: Set<string>,
): Promise<string[]> {
	const cacheNames = await caches.keys();
	const deleted: string[] = [];
	for (const cacheName of cacheNames) {
		if (
			!cacheName.startsWith(APP_SHELL_CACHE_PREFIX) ||
			isTransientBuildCacheName(cacheName)
		)
			continue;
		const buildId = buildIdFromCacheName(cacheName);
		if (
			!buildId ||
			buildId === BUILD_ID ||
			!retiredBuilds.has(buildId) ||
			!canDeleteBuildCache(buildId, owners)
		) {
			continue;
		}
		if (await caches.delete(cacheName)) {
			deleted.push(cacheName);
			await setRetiredBuild(buildId, false);
		}
	}
	return deleted;
}

async function reconcileOwnership(): Promise<string[]> {
	const windows = await self.clients.matchAll({
		type: "window",
		includeUncontrolled: true,
	});
	const owners = await readOwners();
	const retiredBuilds = await readRetiredBuilds();
	const liveClientIds = new Set(windows.map((client) => client.id));
	let hasUndeclaredClient = false;
	const releasedBuilds = new Set<string>();

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
				releasedBuilds.add(owners[clientId]);
				await deleteOwner(clientId);
				delete owners[clientId];
			}),
	);
	for (const buildId of releasedBuilds) {
		if (!Object.values(owners).includes(buildId)) {
			retiredBuilds.add(buildId);
			await setRetiredBuild(buildId, true);
		}
	}
	for (const clientId of pendingNavigationBuilds.keys()) {
		if (!liveClientIds.has(clientId)) pendingNavigationBuilds.delete(clientId);
	}

	if (!hasUndeclaredClient) {
		return cleanupUnownedBuildCaches(owners, retiredBuilds);
	}
	return [];
}

async function copyCache(
	sourceName: string,
	targetName: string,
): Promise<void> {
	const source = await caches.open(sourceName);
	const target = await caches.open(targetName);
	for (const request of await source.keys()) {
		const response = await source.match(request);
		if (!response)
			throw new Error(`cache source entry disappeared: ${request.url}`);
		await target.put(request, response);
	}
	for (const request of await source.keys()) {
		if (!(await target.match(request)))
			throw new Error(`cache target entry missing: ${request.url}`);
	}
}

async function readBuildManifest(
	cacheName: string,
	expectedBuildId: string,
): Promise<Set<string> | null> {
	if (!(await caches.has(cacheName))) return null;
	const cache = await caches.open(cacheName);
	const metadataResponse = await cache.match(BUILD_METADATA_URL);
	if (!metadataResponse) return null;

	let metadata: BuildMetadata;
	try {
		const candidate = (await metadataResponse.json()) as Partial<BuildMetadata>;
		if (
			typeof candidate.buildId !== "string" ||
			!Array.isArray(candidate.entries) ||
			candidate.entries.length === 0 ||
			candidate.entries.some(
				(entry) =>
					typeof entry?.url !== "string" ||
					(entry.revision !== null && typeof entry.revision !== "string"),
			) ||
			candidate.buildId !== expectedBuildId
		) {
			return null;
		}
		metadata = {
			buildId: candidate.buildId,
			entries: candidate.entries,
		};
	} catch {
		return null;
	}

	if (
		expectedBuildId === BUILD_ID &&
		JSON.stringify(metadata.entries) !==
			JSON.stringify(normalizedManifestEntries)
	) {
		return null;
	}
	const urls = new Set(metadata.entries.map((entry) => entry.url));
	for (const url of urls) {
		if (!(await cache.match(url))) return null;
	}
	return urls;
}

async function cacheContainsManifest(cacheName: string): Promise<boolean> {
	return (await readBuildManifest(cacheName, BUILD_ID)) !== null;
}

async function writeBuildMetadata(
	cache: Cache,
	buildId: string,
): Promise<void> {
	await cache.put(
		BUILD_METADATA_URL,
		new Response(
			JSON.stringify({ buildId, entries: normalizedManifestEntries }),
			{
				headers: { "Content-Type": "application/json" },
			},
		),
	);
}

async function fetchManifestEntry(entry: {
	url: string;
	revision: string | null;
}): Promise<Response> {
	const requestUrl = new URL(entry.url);
	if (entry.revision)
		requestUrl.searchParams.set("__WB_REVISION__", entry.revision);
	const response = await fetch(new Request(requestUrl, { cache: "reload" }));
	if (!response.ok)
		throw new Error(`precache failed: ${entry.url} (${response.status})`);
	const contentType = response.headers.get("Content-Type")?.toLowerCase() ?? "";
	const expectsHtml = new URL(entry.url).pathname.endsWith(".html");
	if (
		expectsHtml
			? !contentType.includes("text/html")
			: contentType.includes("text/html")
	) {
		throw new Error(`precache content type mismatch: ${entry.url}`);
	}
	return response;
}

function runCacheMutation<T>(operation: () => Promise<T>): Promise<T> {
	const next = cacheMutation.then(operation, operation);
	cacheMutation = next.then(
		() => undefined,
		() => undefined,
	);
	return next;
}

async function replaceCacheFromStaging(
	sourceName: string,
	targetName: string,
): Promise<boolean> {
	return runCacheMutation(async () => {
		// Complete build caches are immutable. An incomplete target is never
		// served and may be rebuilt after the staged replacement is ready.
		if (await caches.has(targetName)) {
			if (await cacheContainsManifest(targetName)) return true;
			if (!(await caches.delete(targetName))) return false;
		}

		try {
			await copyCache(sourceName, targetName);
			if (!(await cacheContainsManifest(targetName))) {
				throw new Error("precache target is incomplete");
			}
			return true;
		} catch {
			// No controlled client can own a build before its first complete
			// install, so an unpublished partial target can be discarded safely.
			await caches.delete(targetName);
			return false;
		}
	});
}

async function completeBuildCache(): Promise<boolean> {
	if (await cacheContainsManifest(CACHE_NAME)) return true;

	const recoveryName = `${CACHE_NAME}-recovery-${Date.now()}`;
	const recovery = await caches.open(recoveryName);
	try {
		for (const entry of normalizedManifestEntries) {
			await recovery.put(entry.url, await fetchManifestEntry(entry));
		}
		await writeBuildMetadata(recovery, BUILD_ID);
		if (!(await replaceCacheFromStaging(recoveryName, CACHE_NAME)))
			return false;
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
		for (const entry of normalizedManifestEntries) {
			await staging.put(entry.url, await fetchManifestEntry(entry));
		}
		for (const url of manifestUrls) {
			if (!(await staging.match(url)))
				throw new Error(`precache verification failed: ${url}`);
		}
		await writeBuildMetadata(staging, BUILD_ID);
		if (!(await replaceCacheFromStaging(stagingName, CACHE_NAME))) {
			throw new Error("precache replacement failed");
		}
		await caches.delete(stagingName);
	} catch (error) {
		await caches.delete(stagingName);
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
	const assetUrl = new URL(requestUrl.href);
	assetUrl.search = "";
	const requestKind = event.request.mode === "navigate" ? "navigate" : "asset";
	const requestClientId = event.clientId || event.resultingClientId || null;
	if (requestKind === "navigate") {
		if (event.clientId) pendingNavigationBuilds.set(event.clientId, BUILD_ID);
		if (event.resultingClientId) {
			pendingNavigationBuilds.set(event.resultingClientId, BUILD_ID);
		}
	}
	const owners = await readOwners();
	const hintedBuild = requestUrl.searchParams.get("xp-build");
	const availableHintedBuild =
		hintedBuild &&
		(await readBuildManifest(appShellCacheName(hintedBuild), hintedBuild))
			? hintedBuild
			: null;
	const selectedClientBuild = requestClientId
		? (pendingNavigationBuilds.get(requestClientId) ?? owners[requestClientId])
		: null;
	const buildId = selectBuildForRequest({
		requestMode: requestKind,
		activeBuildId: BUILD_ID,
		clientBuildId: selectedClientBuild ?? availableHintedBuild,
	});
	const responseUrl =
		requestKind === "navigate"
			? new URL("/index.html", self.registration.scope).href
			: assetUrl.href;
	if (!buildId) {
		await respondToClient(event.clientId || null, {
			type: "XP_REQUEST_BUILD_DECLARATION",
			buildId: BUILD_ID,
			url: requestUrl.pathname,
		});
		return new Response("The application build has not been declared yet.", {
			status: 504,
			headers: { "Content-Type": "text/plain" },
		});
	}
	const selectedManifest = await readBuildManifest(
		appShellCacheName(buildId),
		buildId,
	);
	if (!selectedManifest) {
		await respondToClient(event.clientId || null, {
			type: "XP_CACHE_MISS",
			buildId,
			url: responseUrl,
		});
		return new Response("The selected application build is incomplete.", {
			status: 504,
			headers: { "Content-Type": "text/plain" },
		});
	}
	if (requestKind === "asset" && !selectedManifest.has(assetUrl.href)) {
		await respondToClient(event.clientId || null, {
			type: "XP_CACHE_MISS",
			buildId,
			url: assetUrl.href,
		});
		return new Response("Static asset is not part of the selected precache.", {
			status: 504,
			headers: { "Content-Type": "text/plain" },
		});
	}
	const cache = await caches.open(appShellCacheName(buildId));
	const response = await cache.match(responseUrl, { ignoreSearch: true });
	if (response) return response;

	await respondToClient(event.clientId || null, {
		type: "XP_CACHE_MISS",
		buildId,
		url: responseUrl,
	});
	return new Response("The selected application build is incomplete.", {
		status: 504,
		headers: { "Content-Type": "text/plain" },
	});
}

self.addEventListener("install", (event) => {
	event.waitUntil(
		(async () => {
			const existingBuilds = await caches.keys();
			shouldClaimClients = !existingBuilds.some(
				(cacheName) =>
					cacheName.startsWith(APP_SHELL_CACHE_PREFIX) &&
					cacheName !== CACHE_NAME,
			);
			await installBuild();
		})(),
	);
});

self.addEventListener("activate", (event) => {
	event.waitUntil(
		(async () => {
			if (shouldClaimClients) await self.clients.claim();
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
				const ownersBeforeDeclaration = await readOwners();
				const cacheAvailable =
					(await readBuildManifest(
						appShellCacheName(message.buildId),
						message.buildId,
					)) !== null;
				if (!cacheAvailable) {
					await respondToClient(clientId, {
						type: "XP_CACHE_MISS",
						buildId: message.buildId,
					});
					return;
				}
				await setRetiredBuild(message.buildId, false);
				await writeOwner(clientId, message.buildId);
				const previousBuildId = ownersBeforeDeclaration[clientId];
				const remainingOwners = Object.fromEntries(
					Object.entries(ownersBeforeDeclaration).filter(
						([ownerClientId]) => ownerClientId !== clientId,
					),
				);
				await retireBuildIfUnowned(previousBuildId, remainingOwners);
				pendingNavigationBuilds.delete(clientId);
				await reconcileOwnership();
				return;
			}
			if (message?.type === "XP_RELEASE_BUILD" && clientId) {
				const ownersBeforeRelease = await readOwners();
				await deleteOwner(clientId);
				const remainingOwners = Object.fromEntries(
					Object.entries(ownersBeforeRelease).filter(
						([ownerClientId]) => ownerClientId !== clientId,
					),
				);
				await retireBuildIfUnowned(
					ownersBeforeRelease[clientId],
					remainingOwners,
				);
				pendingNavigationBuilds.delete(clientId);
				await reconcileOwnership();
				return;
			}
			if (message?.type === "XP_REQUEST_CACHE_RECOVERY") {
				const complete =
					(message.buildId === "active" || message.buildId === BUILD_ID) &&
					(await completeBuildCache());
				const deleted = complete ? await reconcileOwnership() : [];
				await respondToClient(clientId, {
					type: complete
						? "XP_CACHE_RECOVERY_READY"
						: "XP_CACHE_RECOVERY_UNAVAILABLE",
					buildId: BUILD_ID,
					deleted,
				});
			}
		})(),
	);
});
