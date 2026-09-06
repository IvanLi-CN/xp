const RUNTIME_POLICY_SESSION_KEY = "xp-runtime-policy-applied";

export function readAppliedRuntimePolicyId(): string | null {
	if (typeof window === "undefined") return null;
	try {
		return window.sessionStorage.getItem(RUNTIME_POLICY_SESSION_KEY);
	} catch {
		return null;
	}
}

export function writeAppliedRuntimePolicyId(policyId: string): void {
	if (typeof window === "undefined") return;
	try {
		window.sessionStorage.setItem(RUNTIME_POLICY_SESSION_KEY, policyId);
	} catch {
		// Session storage is optional; the policy remains valid for this page.
	}
}

export function clearAppliedRuntimePolicyId(): void {
	if (typeof window === "undefined") return;
	try {
		window.sessionStorage.removeItem(RUNTIME_POLICY_SESSION_KEY);
	} catch {
		// Session storage is optional.
	}
}
