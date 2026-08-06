export const APP_SHELL_CACHE_PREFIX = "xp-app-shell-";
export const LEGACY_WORKBOX_PRECACHE_PREFIX = "workbox-precache-v2-";

export type PwaRequestKind = "navigate" | "asset";
export type WorkerInstallMode =
	| "first_install"
	| "legacy_migration"
	| "normal_update";

export function appShellCacheName(buildId: string): string {
	return `${APP_SHELL_CACHE_PREFIX}${buildId}`;
}

export function legacyWorkboxPrecacheCacheName(scope: string): string {
	return `${LEGACY_WORKBOX_PRECACHE_PREFIX}${scope}`;
}

export function selectWorkerInstallMode(args: {
	cacheNames: readonly string[];
	currentCacheName: string;
	legacyCacheName: string;
	liveClientCount: number;
	validXpOwnerCount: number;
}): WorkerInstallMode {
	const hasLegacyPrecache = args.cacheNames.includes(args.legacyCacheName);
	if (
		hasLegacyPrecache &&
		args.liveClientCount > 0 &&
		args.validXpOwnerCount === 0
	) {
		return "legacy_migration";
	}

	const hasPreviousAppShell = args.cacheNames.some(
		(cacheName) =>
			cacheName.startsWith(APP_SHELL_CACHE_PREFIX) &&
			cacheName !== args.currentCacheName &&
			!isTransientBuildCacheName(cacheName),
	);
	return hasPreviousAppShell || hasLegacyPrecache
		? "normal_update"
		: "first_install";
}

export function selectBuildForRequest(args: {
	requestMode: PwaRequestKind;
	activeBuildId: string;
	clientBuildId?: string | null;
}): string | null {
	if (args.requestMode === "navigate") return args.activeBuildId;
	return args.clientBuildId ?? null;
}

export function canDeleteBuildCache(
	buildId: string,
	owners: Record<string, string>,
): boolean {
	return Object.values(owners).every(
		(ownerBuildId) => ownerBuildId !== buildId,
	);
}

export function isTransientBuildCacheName(cacheName: string): boolean {
	return (
		cacheName.includes("-install-") ||
		cacheName.includes("-recovery-") ||
		cacheName.includes("-backup-")
	);
}

export function buildIdFromCacheName(cacheName: string): string | null {
	return cacheName.startsWith(APP_SHELL_CACHE_PREFIX)
		? cacheName.slice(APP_SHELL_CACHE_PREFIX.length) || null
		: null;
}
