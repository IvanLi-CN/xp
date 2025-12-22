export const ADMIN_TOKEN_STORAGE_KEY = "xp_admin_token";

export function readAdminToken(): string {
	try {
		return localStorage.getItem(ADMIN_TOKEN_STORAGE_KEY) ?? "";
	} catch {
		return "";
	}
}

export function writeAdminToken(token: string): void {
	try {
		localStorage.setItem(ADMIN_TOKEN_STORAGE_KEY, token);
	} catch {
		// ignore storage errors in unsupported environments
	}
}

export function clearAdminToken(): void {
	try {
		localStorage.removeItem(ADMIN_TOKEN_STORAGE_KEY);
	} catch {
		// ignore storage errors in unsupported environments
	}
}

export function hasAdminToken(): boolean {
	return readAdminToken().length > 0;
}
