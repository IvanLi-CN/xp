import type { RuntimePolicy } from "../api/runtimePolicy";

export type RuntimePolicyGrant = Pick<
	RuntimePolicy,
	"policy_id" | "cluster_id" | "expires_at" | "api_origins"
>;

export function setServiceWorkerRuntimePolicy(
	policy: RuntimePolicyGrant,
): void {
	if (typeof navigator === "undefined" || !("serviceWorker" in navigator)) {
		return;
	}
	const message = {
		type: "XP_SET_RUNTIME_POLICY",
		policy,
	} as const;
	navigator.serviceWorker.controller?.postMessage(message);
	void navigator.serviceWorker.ready.then((registration) => {
		registration.active?.postMessage(message);
	});
}

export function clearServiceWorkerRuntimePolicy(): void {
	if (typeof navigator === "undefined" || !("serviceWorker" in navigator)) {
		return;
	}
	const message = { type: "XP_CLEAR_RUNTIME_POLICY" } as const;
	navigator.serviceWorker.controller?.postMessage(message);
	void navigator.serviceWorker.ready.then((registration) => {
		registration.active?.postMessage(message);
	});
}
