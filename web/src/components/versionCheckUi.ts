import type { VersionCheckResponse } from "../api/versionCheck";

export const XP_GITHUB_REPO = "IvanLi-CN/xp";

export const VERSION_CHECK_LAST_AT_STORAGE_KEY = "xp_version_check_last_at_ms";
export const VERSION_CHECK_COOLDOWN_MS = 60 * 60 * 1000;

export type VersionCheckUiState =
	| { kind: "idle" }
	| { kind: "checking" }
	| {
			kind: "up_to_date";
			latest_tag: string;
			checked_at: string;
			comparable: boolean;
			repo: string;
	  }
	| {
			kind: "update_available";
			latest_tag: string;
			checked_at: string;
			repo: string;
	  }
	| { kind: "check_failed"; message: string };

export type VersionCheckUiEvent =
	| { type: "start" }
	| { type: "success"; data: VersionCheckResponse }
	| { type: "fail"; message: string };

export function reduceVersionCheckUiState(
	_prev: VersionCheckUiState,
	event: VersionCheckUiEvent,
): VersionCheckUiState {
	if (event.type === "start") return { kind: "checking" };

	if (event.type === "fail") {
		return { kind: "check_failed", message: event.message };
	}

	const { data } = event;
	const repo = data.source.repo || XP_GITHUB_REPO;
	const comparable = data.has_update !== null;
	if (data.has_update === true) {
		return {
			kind: "update_available",
			latest_tag: data.latest.release_tag,
			checked_at: data.checked_at,
			repo,
		};
	}

	return {
		kind: "up_to_date",
		latest_tag: data.latest.release_tag,
		checked_at: data.checked_at,
		comparable,
		repo,
	};
}

function safeLocalStorageGet(key: string): string | null {
	try {
		return localStorage.getItem(key);
	} catch {
		return null;
	}
}

function safeLocalStorageSet(key: string, value: string): void {
	try {
		localStorage.setItem(key, value);
	} catch {
		// ignore
	}
}

export function readVersionCheckLastAtMs(): number | null {
	const raw = safeLocalStorageGet(VERSION_CHECK_LAST_AT_STORAGE_KEY);
	if (!raw) return null;
	const value = Number(raw);
	if (!Number.isFinite(value) || value <= 0) return null;
	return value;
}

export function writeVersionCheckLastAtMs(value: number): void {
	if (!Number.isFinite(value) || value <= 0) return;
	safeLocalStorageSet(
		VERSION_CHECK_LAST_AT_STORAGE_KEY,
		String(Math.floor(value)),
	);
}

export function shouldAutoCheckVersion(args: {
	nowMs: number;
	lastAtMs: number | null;
	cooldownMs?: number;
}): boolean {
	const cooldownMs = args.cooldownMs ?? VERSION_CHECK_COOLDOWN_MS;
	if (args.lastAtMs === null) return true;
	return args.nowMs - args.lastAtMs >= cooldownMs;
}

const SEMVERISH_RE = /^v?(?<ver>\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/;

export function toSemverishReleaseTag(version: string): string | null {
	const match = SEMVERISH_RE.exec(version.trim());
	const ver = match?.groups?.ver;
	return ver ? `v${ver}` : null;
}

export function githubReleasesUrl(repo: string = XP_GITHUB_REPO): string {
	return `https://github.com/${repo}/releases`;
}

export function githubReleaseTagUrl(
	tag: string,
	repo: string = XP_GITHUB_REPO,
): string {
	return `https://github.com/${repo}/releases/tag/${encodeURIComponent(tag)}`;
}

export function xpVersionLinkHref(
	xpVersion: string | null | undefined,
	repo?: string | null,
): string {
	const effectiveRepo = repo ?? XP_GITHUB_REPO;
	if (!xpVersion) return githubReleasesUrl(effectiveRepo);
	const tag = toSemverishReleaseTag(xpVersion);
	return tag
		? githubReleaseTagUrl(tag, effectiveRepo)
		: githubReleasesUrl(effectiveRepo);
}
