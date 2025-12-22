import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminJoinTokenResponseSchema = z.object({
	join_token: z.string(),
});

export type AdminJoinTokenResponse = z.infer<
	typeof AdminJoinTokenResponseSchema
>;

export type AdminJoinTokenRequest = {
	ttl_seconds: number;
};

export async function createAdminJoinToken(
	adminToken: string,
	payload: AdminJoinTokenRequest,
	signal?: AbortSignal,
): Promise<AdminJoinTokenResponse> {
	const res = await fetch("/api/admin/cluster/join-tokens", {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify(payload),
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminJoinTokenResponseSchema.parse(json);
}
