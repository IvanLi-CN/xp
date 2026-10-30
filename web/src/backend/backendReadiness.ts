import type { PrimaryBackendSnapshot } from "./primaryBackendModel";

type AuthenticatedBackendReadiness = {
	origin: string;
	clusterId: string | null;
	ready: boolean;
};

let authenticatedBackendReadiness: AuthenticatedBackendReadiness | null = null;

export function invalidateAuthenticatedBackendReadiness() {
	if (authenticatedBackendReadiness) {
		authenticatedBackendReadiness = {
			...authenticatedBackendReadiness,
			ready: false,
		};
	}
}

export function setAuthenticatedBackendReadiness(
	readiness: AuthenticatedBackendReadiness | null,
) {
	authenticatedBackendReadiness = readiness;
}

export function clearAuthenticatedBackendReadiness() {
	authenticatedBackendReadiness = null;
}

export function isAuthenticatedBackendBlocked(
	state: PrimaryBackendSnapshot,
): boolean {
	return Boolean(
		authenticatedBackendReadiness &&
			(!authenticatedBackendReadiness.ready ||
				authenticatedBackendReadiness.origin !== state.primaryOrigin ||
				authenticatedBackendReadiness.clusterId !== state.clusterId),
	);
}
