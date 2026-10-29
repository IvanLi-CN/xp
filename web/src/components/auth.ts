import { clearPrimaryBackendDirectory } from "../backend/primaryBackend";
import {
	isDemoRuntime,
	readDemoAuthentication,
	setDemoAuthentication,
} from "../demo/runtimeMode";
import { clearAppliedRuntimePolicyId } from "../offline/runtimePolicySession";
import { clearServiceWorkerRuntimePolicy } from "../offline/serviceWorkerPolicy";

export const ADMIN_TOKEN_STORAGE_KEY = "xp_admin_token";

function clearSessionState(): void {
	clearPrimaryBackendDirectory({ clearProfiles: true });
	clearAppliedRuntimePolicyId();
	clearServiceWorkerRuntimePolicy();
}

if (typeof window !== "undefined") {
	window.addEventListener("storage", (event) => {
		if (
			event.key === null ||
			(event.key === ADMIN_TOKEN_STORAGE_KEY &&
				event.oldValue !== event.newValue)
		) {
			clearSessionState();
		}
	});
}

export function readAdminToken(): string {
	if (isDemoRuntime()) return readDemoAuthentication() ? "xp-demo-admin" : "";
	try {
		return localStorage.getItem(ADMIN_TOKEN_STORAGE_KEY) ?? "";
	} catch {
		return "";
	}
}

export function writeAdminToken(token: string): void {
	if (readAdminToken() !== token) clearSessionState();
	if (isDemoRuntime()) {
		setDemoAuthentication(token.length > 0);
		return;
	}
	try {
		localStorage.setItem(ADMIN_TOKEN_STORAGE_KEY, token);
	} catch {
		// ignore storage errors in unsupported environments
	}
}

export function clearAdminToken(): void {
	clearSessionState();
	if (isDemoRuntime()) {
		setDemoAuthentication(false);
		return;
	}
	try {
		localStorage.removeItem(ADMIN_TOKEN_STORAGE_KEY);
	} catch {
		// ignore storage errors in unsupported environments
	}
}

export function hasAdminToken(): boolean {
	return readAdminToken().length > 0;
}
