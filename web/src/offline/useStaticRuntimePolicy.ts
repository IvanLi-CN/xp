import { useEffect } from "react";

import type { RuntimePolicy } from "../api/runtimePolicy";
import {
	STATIC_BOOTSTRAP_ORIGIN,
	getStaticRuntimePolicyOrigins,
	hydrateStaticRuntimePolicy,
} from "../backend/primaryBackend";
import { readAppliedRuntimePolicyId } from "./runtimePolicySession";
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
		const candidateOrigins = [
			...apiOrigins,
			...args.policy.node_states.map((node) => node.origin),
		];
		hydrateStaticRuntimePolicy(args.clusterId, apiOrigins, {
			// Include temporarily unavailable nodes as candidates, but keep them out
			// of the active Service Worker/CSP grant below.
			candidateOrigins,
			nodeStates: args.policy.node_states,
			policyId: args.policy.policy_id,
			policyGrant: {
				policy_id: args.policy.policy_id,
				cluster_id: args.policy.cluster_id,
				expires_at: args.policy.expires_at,
				api_origins: [...new Set(apiOrigins)],
			},
			activate: readAppliedRuntimePolicyId() === args.policy.policy_id,
		});
		setServiceWorkerRuntimePolicy({
			policy_id: args.policy.policy_id,
			cluster_id: args.policy.cluster_id,
			expires_at: args.policy.expires_at,
			api_origins: getStaticRuntimePolicyOrigins(),
		});
	}, [args.clusterId, args.enabled, args.policy]);
}
