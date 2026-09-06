import { useEffect } from "react";

import { hydrateStaticRuntimePolicy } from "../backend/primaryBackend";
import { clearAppliedRuntimePolicyId } from "./runtimePolicySession";
import { clearServiceWorkerRuntimePolicy } from "./serviceWorkerPolicy";

export function useRuntimePolicyExpiry(args: {
	enabled: boolean;
	clusterId: string | undefined;
	expiresAt: string | undefined;
}): void {
	useEffect(() => {
		if (!args.enabled || !args.expiresAt) return;
		const delayMs = Math.max(0, Date.parse(args.expiresAt) - Date.now());
		const timer = window.setTimeout(() => {
			if (args.clusterId) hydrateStaticRuntimePolicy(args.clusterId, []);
			clearServiceWorkerRuntimePolicy();
			clearAppliedRuntimePolicyId();
		}, delayMs);
		return () => window.clearTimeout(timer);
	}, [args.clusterId, args.enabled, args.expiresAt]);
}
