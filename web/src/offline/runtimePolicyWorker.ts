import { applyRuntimePolicyToCsp } from "./runtimePolicyCsp";

export type RuntimePolicyGrant = {
	policyId: string;
	clusterId: string;
	expiresAt: number;
	apiOrigins: string[];
};

export type RuntimePolicyMessage =
	| {
			type: "XP_SET_RUNTIME_POLICY";
			policy: {
				policy_id: string;
				cluster_id: string;
				expires_at: string;
				api_origins: string[];
			};
	  }
	| { type: "XP_CLEAR_RUNTIME_POLICY" };

export type ServiceWorkerMessage =
	| { type: "SKIP_WAITING" }
	| { type: "XP_DECLARE_BUILD"; buildId: string }
	| { type: "XP_RELEASE_BUILD" }
	| RuntimePolicyMessage
	| {
			type: "XP_REQUEST_CACHE_RECOVERY";
			buildId: string | "active";
			requestId?: string;
	  };

export function parseRuntimePolicyGrant(
	value: unknown,
	now = Date.now(),
): RuntimePolicyGrant | null {
	if (!value || typeof value !== "object") return null;
	const policy = value as Partial<{
		policy_id: unknown;
		cluster_id: unknown;
		expires_at: unknown;
		api_origins: unknown;
	}>;
	if (
		typeof policy.policy_id !== "string" ||
		policy.policy_id.length === 0 ||
		typeof policy.cluster_id !== "string" ||
		policy.cluster_id.length === 0 ||
		typeof policy.expires_at !== "string" ||
		!Array.isArray(policy.api_origins) ||
		policy.api_origins.length > 128
	)
		return null;
	const expiresAt = Date.parse(policy.expires_at);
	if (!Number.isFinite(expiresAt) || expiresAt <= now) return null;
	const origins: string[] = [];
	for (const rawOrigin of policy.api_origins) {
		if (typeof rawOrigin !== "string") return null;
		try {
			const origin = new URL(rawOrigin);
			if (
				origin.protocol !== "https:" ||
				(origin.pathname !== "" && origin.pathname !== "/") ||
				origin.search ||
				origin.hash ||
				origin.username ||
				origin.password
			)
				return null;
			origins.push(origin.origin);
		} catch {
			return null;
		}
	}
	return {
		policyId: policy.policy_id,
		clusterId: policy.cluster_id,
		expiresAt,
		apiOrigins: [...new Set(origins)],
	};
}

export function applyRuntimePolicyGrant(
	response: Response,
	grant: RuntimePolicyGrant,
	now = Date.now(),
): Response | null {
	if (grant.expiresAt <= now) return null;
	const csp = response.headers.get("Content-Security-Policy");
	if (!csp) return response;
	const headers = new Headers(response.headers);
	headers.set(
		"Content-Security-Policy",
		applyRuntimePolicyToCsp(csp, grant.apiOrigins),
	);
	return new Response(response.body, {
		status: response.status,
		statusText: response.statusText,
		headers,
	});
}
