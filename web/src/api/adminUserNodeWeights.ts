import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminUserNodeWeightItemSchema = z.object({
	node_id: z.string(),
	weight: z.number().int().nonnegative().max(65535),
});

export type AdminUserNodeWeightItem = z.infer<
	typeof AdminUserNodeWeightItemSchema
>;

export const AdminUserNodeWeightsResponseSchema = z.object({
	items: z.array(AdminUserNodeWeightItemSchema),
});

export type AdminUserNodeWeightsResponse = z.infer<
	typeof AdminUserNodeWeightsResponseSchema
>;

export async function fetchAdminUserNodeWeights(
	adminToken: string,
	userId: string,
	signal?: AbortSignal,
): Promise<AdminUserNodeWeightsResponse> {
	const res = await fetch(`/api/admin/users/${userId}/node-weights`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUserNodeWeightsResponseSchema.parse(json);
}

export async function putAdminUserNodeWeight(
	adminToken: string,
	userId: string,
	nodeId: string,
	weight: number,
	signal?: AbortSignal,
): Promise<AdminUserNodeWeightItem> {
	const res = await fetch(`/api/admin/users/${userId}/node-weights/${nodeId}`, {
		method: "PUT",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ weight }),
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUserNodeWeightItemSchema.parse(json);
}
