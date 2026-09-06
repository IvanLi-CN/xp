import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const STATIC_CONSOLE_CAPABILITY = "web.static-console-v1";
export const RUNTIME_POLICY_PATH = "/api/admin/console/runtime-policy";

export const RuntimePolicySchema = z
	.object({
		policy_id: z.string().min(1),
		cluster_id: z.string().min(1),
		issued_at: z.string().datetime({ offset: true }),
		expires_at: z.string().datetime({ offset: true }),
		api_origins: z
			.array(
				z
					.string()
					.url()
					.refine((value) => {
						try {
							const url = new URL(value);
							return (
								url.protocol === "https:" &&
								(url.pathname === "" || url.pathname === "/") &&
								!url.search &&
								!url.hash &&
								!url.username &&
								!url.password
							);
						} catch {
							return false;
						}
					}, "origin must be a canonical HTTPS origin"),
			)
			.max(128),
	})
	.superRefine((policy, context) => {
		const issuedAt = Date.parse(policy.issued_at);
		const expiresAt = Date.parse(policy.expires_at);
		if (!Number.isFinite(issuedAt) || !Number.isFinite(expiresAt)) return;
		if (expiresAt <= issuedAt) {
			context.addIssue({
				code: z.ZodIssueCode.custom,
				path: ["expires_at"],
				message: "policy must expire after it is issued",
			});
		}
		if (expiresAt <= Date.now()) {
			context.addIssue({
				code: z.ZodIssueCode.custom,
				path: ["expires_at"],
				message: "policy has expired",
			});
		}
	});

export type RuntimePolicy = z.infer<typeof RuntimePolicySchema>;

export async function fetchRuntimePolicy(
	adminToken: string,
	signal?: AbortSignal,
): Promise<RuntimePolicy> {
	const response = await fetch(RUNTIME_POLICY_PATH, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});
	await throwIfNotOk(response);
	return RuntimePolicySchema.parse(await response.json());
}
