import { isDemoRuntime } from "../demo/runtimeMode";
import type { BackendProfile } from "./primaryBackendModel";

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
