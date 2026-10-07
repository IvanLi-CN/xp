import type { AdminUserMihomoProfile } from "../api/adminUsers";

const mihomoSaveQueues = new Map<string, Promise<void>>();
const mihomoSaveGenerations = new Map<string, number>();
const MIHOMO_LOCK_REQUIRED_ERROR =
	"Mihomo profile saving requires browser locking support. Retry in a supported browser.";
type MihomoProfileSaveState = {
	successfulSaveEpoch: number;
	latestSuccessfulProfile?: AdminUserMihomoProfile;
};
const mihomoProfileSaveStates = new Map<string, MihomoProfileSaveState>();

function mihomoProfileSaveState(saveKey: string): MihomoProfileSaveState {
	const current = mihomoProfileSaveStates.get(saveKey);
	if (current) return current;
	const initial: MihomoProfileSaveState = { successfulSaveEpoch: 0 };
	mihomoProfileSaveStates.set(saveKey, initial);
	return initial;
}

export function nextMihomoSaveGeneration(saveKey: string): number {
	const next = (mihomoSaveGenerations.get(saveKey) ?? 0) + 1;
	mihomoSaveGenerations.set(saveKey, next);
	return next;
}

export function isCurrent(saveKey: string, generation: number): boolean {
	return mihomoSaveGenerations.get(saveKey) === generation;
}

export function recordMihomoProfileSave(
	saveKey: string,
	profile: AdminUserMihomoProfile,
): void {
	const state = mihomoProfileSaveState(saveKey);
	state.successfulSaveEpoch += 1;
	state.latestSuccessfulProfile = { ...profile };
}

export async function fetchMihomoProfileWithSaveSnapshot(
	saveKey: string,
	fetchProfile: () => Promise<AdminUserMihomoProfile>,
): Promise<AdminUserMihomoProfile> {
	const state = mihomoProfileSaveState(saveKey);
	const successfulSaveEpoch = state.successfulSaveEpoch;
	const profile = await fetchProfile();
	const latestState = mihomoProfileSaveState(saveKey);
	if (
		latestState.successfulSaveEpoch > successfulSaveEpoch &&
		latestState.latestSuccessfulProfile
	) {
		return { ...latestState.latestSuccessfulProfile };
	}
	return profile;
}

function mihomoProfilesEqual(
	left: AdminUserMihomoProfile,
	right: AdminUserMihomoProfile,
): boolean {
	return (
		left.mixin_yaml === right.mixin_yaml &&
		left.extra_proxies_yaml === right.extra_proxies_yaml &&
		left.extra_proxy_providers_yaml === right.extra_proxy_providers_yaml
	);
}

export async function ensureMihomoProfileBaseline(
	baseline: AdminUserMihomoProfile,
	fetchProfile: () => Promise<AdminUserMihomoProfile>,
	onConflict: (latestProfile: AdminUserMihomoProfile) => void,
): Promise<void> {
	const latestProfile = await fetchProfile();
	if (mihomoProfilesEqual(latestProfile, baseline)) return;
	onConflict(latestProfile);
	throw new Error(
		"Mihomo profile changed elsewhere. Refresh the profile and review your draft before retrying.",
	);
}

export function enqueueMihomoSave<T>(
	saveKey: string,
	lockKey: string,
	operation: () => Promise<T>,
): Promise<T> {
	const previous = mihomoSaveQueues.get(saveKey) ?? Promise.resolve();
	const run = async () => {
		if (typeof navigator !== "undefined" && navigator.locks) {
			return await navigator.locks.request<Promise<T>>(
				`xp-mihomo-profile:${lockKey}`,
				operation,
			);
		}
		if (typeof window !== "undefined") {
			throw new Error(MIHOMO_LOCK_REQUIRED_ERROR);
		}
		return operation();
	};
	const current = previous.catch(() => undefined).then(run);
	const settled = current.then(
		() => undefined,
		() => undefined,
	);
	mihomoSaveQueues.set(saveKey, settled);
	return current.finally(() => {
		if (mihomoSaveQueues.get(saveKey) === settled) {
			mihomoSaveQueues.delete(saveKey);
		}
	});
}
