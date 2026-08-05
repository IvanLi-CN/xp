export const APP_SHELL_CACHE_PREFIX = "xp-app-shell-";

export type PwaRequestKind = "navigate" | "asset";

export function appShellCacheName(buildId: string): string {
	return `${APP_SHELL_CACHE_PREFIX}${buildId}`;
}

export function selectBuildForRequest(args: {
	requestMode: PwaRequestKind;
	activeBuildId: string;
	clientBuildId?: string | null;
}): string {
	if (args.requestMode === "navigate") return args.activeBuildId;
	return args.clientBuildId || args.activeBuildId;
}

export function canDeleteBuildCache(
	buildId: string,
	owners: Record<string, string>,
): boolean {
	return Object.values(owners).every(
		(ownerBuildId) => ownerBuildId !== buildId,
	);
}

export function buildIdFromCacheName(cacheName: string): string | null {
	return cacheName.startsWith(APP_SHELL_CACHE_PREFIX)
		? cacheName.slice(APP_SHELL_CACHE_PREFIX.length) || null
		: null;
}
