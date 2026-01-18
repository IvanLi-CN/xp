import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminUserNodeQuotaSchema = z.object({
	user_id: z.string(),
	node_id: z.string(),
	quota_limit_bytes: z.number().int().nonnegative(),
});

export type AdminUserNodeQuota = z.infer<typeof AdminUserNodeQuotaSchema>;

export const AdminUserNodeQuotasResponseSchema = z.object({
	items: z.array(AdminUserNodeQuotaSchema),
});

export type AdminUserNodeQuotasResponse = z.infer<
	typeof AdminUserNodeQuotasResponseSchema
>;

export async function fetchAdminUserNodeQuotas(
	adminToken: string,
	userId: string,
	signal?: AbortSignal,
): Promise<AdminUserNodeQuotasResponse> {
	const res = await fetch(`/api/admin/users/${userId}/node-quotas`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUserNodeQuotasResponseSchema.parse(json);
}

export async function putAdminUserNodeQuota(
	adminToken: string,
	userId: string,
	nodeId: string,
	quotaLimitBytes: number,
	signal?: AbortSignal,
): Promise<AdminUserNodeQuota> {
	const res = await fetch(`/api/admin/users/${userId}/node-quotas/${nodeId}`, {
		method: "PUT",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ quota_limit_bytes: quotaLimitBytes }),
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUserNodeQuotaSchema.parse(json);
}
