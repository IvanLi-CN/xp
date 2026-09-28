import { z } from "zod";

import { QuotaResetSourceSchema } from "./quotaReset";
import { fetchJsonWithTimeout } from "./request";

export const AdminUserNodeQuotaStatusItemSchema = z.object({
	user_id: z.string(),
	node_id: z.string(),
	quota_limit_bytes: z.number().int().nonnegative(),
	used_bytes: z.number().int().nonnegative(),
	remaining_bytes: z.number().int().nonnegative(),
	cycle_end_at: z.string().nullable(),
	quota_reset_source: QuotaResetSourceSchema,
});

export type AdminUserNodeQuotaStatusItem = z.infer<
	typeof AdminUserNodeQuotaStatusItemSchema
>;

export const AdminUserNodeQuotaStatusResponseSchema = z.object({
	partial: z.boolean(),
	unreachable_nodes: z.array(z.string()),
	items: z.array(AdminUserNodeQuotaStatusItemSchema),
});

export type AdminUserNodeQuotaStatusResponse = z.infer<
	typeof AdminUserNodeQuotaStatusResponseSchema
>;

export async function fetchAdminUserNodeQuotaStatus(
	adminToken: string,
	userId: string,
	signal?: AbortSignal,
): Promise<AdminUserNodeQuotaStatusResponse> {
	return fetchJsonWithTimeout(
		`/api/admin/users/${userId}/node-quotas/status`,
		{
			method: "GET",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
		(value) => AdminUserNodeQuotaStatusResponseSchema.parse(value),
	);
}
