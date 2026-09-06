import { useEffect } from "react";

import type { RuntimePolicy } from "../api/runtimePolicy";
import {
	STATIC_BOOTSTRAP_ORIGIN,
	hydrateStaticRuntimePolicy,
} from "../backend/primaryBackend";
import {
	readAppliedRuntimePolicyId,
	writeAppliedRuntimePolicyId,
} from "./runtimePolicySession";
import { setServiceWorkerRuntimePolicy } from "./serviceWorkerPolicy";

export function useStaticRuntimePolicy(args: {
	enabled: boolean;
	clusterId: string | undefined;
	policy: RuntimePolicy | undefined;
}): void {
	useEffect(() => {
		if (!args.enabled || !args.clusterId || !args.policy) return;
		if (args.policy.cluster_id !== args.clusterId) return;
		const apiOrigins = [STATIC_BOOTSTRAP_ORIGIN, ...args.policy.api_origins];
		hydrateStaticRuntimePolicy(args.clusterId, apiOrigins);
		setServiceWorkerRuntimePolicy({
			policy_id: args.policy.policy_id,
			cluster_id: args.policy.cluster_id,
			expires_at: args.policy.expires_at,
			api_origins: [...new Set(apiOrigins)],
		});

		const appliedPolicyId = readAppliedRuntimePolicyId();
		if (appliedPolicyId === args.policy.policy_id) return;
		writeAppliedRuntimePolicyId(args.policy.policy_id);
		void navigator.serviceWorker.ready
			.then(() => window.location.reload())
			.catch(() => undefined);
	}, [args.clusterId, args.enabled, args.policy]);
}
