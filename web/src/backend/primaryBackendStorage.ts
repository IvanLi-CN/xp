import { isDemoRuntime } from "../demo/runtimeMode";
import {
	type BackendProfile,
	type PrimaryBackendSnapshot,
	STATIC_BOOTSTRAP_ORIGIN,
	canonicalBackendOrigin,
	displayNodeName,
	getPageOrigin,
} from "./primaryBackendModel";

export type StoredProfiles = {
	activeClusterId?: string;
	profiles?: Record<string, BackendProfile>;
};

export const STORAGE_KEY = "xp-primary-backend-profiles";

export function readStoredProfiles(isStatic: boolean): StoredProfiles {
	if (isDemoRuntime() || isStatic || typeof window === "undefined") return {};
	try {
		const value = window.localStorage.getItem(STORAGE_KEY);
		if (!value) return {};
		const parsed = JSON.parse(value) as StoredProfiles;
		return parsed && typeof parsed === "object" ? parsed : {};
	} catch {
		return {};
	}
}

export function writeStoredProfiles(value: StoredProfiles, isStatic: boolean) {
	if (isDemoRuntime() || isStatic || typeof window === "undefined") return;
	try {
		window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
	} catch {
		// Private browsing and quota errors must not make the console unusable.
	}
}

export function readInitialState(isStatic: boolean): PrimaryBackendSnapshot {
	const stored = readStoredProfiles(isStatic);
	const active = stored.activeClusterId
		? stored.profiles?.[stored.activeClusterId]
		: undefined;
	const pageOrigin = getPageOrigin();
	const storedPrimary = canonicalBackendOrigin(active?.primaryOrigin ?? "");
	const storedCandidateOrigins = new Set(
		(active?.candidates ?? [])
			.map((candidate) => canonicalBackendOrigin(candidate.origin))
			.filter((origin): origin is string => Boolean(origin)),
	);
	return {
		clusterId: stored.activeClusterId ?? null,
		primaryOrigin: isStatic
			? STATIC_BOOTSTRAP_ORIGIN
			: storedPrimary && storedCandidateOrigins.has(storedPrimary)
				? storedPrimary
				: pageOrigin,
		candidates:
			active?.candidates?.map((candidate) => ({
				...candidate,
				nodeName: displayNodeName(candidate.nodeName, candidate.origin),
			})) ?? [],
		generation: 0,
		state: "ready",
		pendingMutations: 0,
		unknownMutationCount: 0,
		lastSwitchTimedOut: false,
	};
}
