import { z } from "zod";

import { fetchJsonWithTimeout } from "./request";

export const AdminUserQuotaSummaryItemSchema = z.object({
	user_id: z.string(),
	quota_limit_kind: z.enum([
		"unlimited",
		"fixed",
		"shared_base",
		"shared_opportunistic",
		"mixed",
	]),
	quota_limit_bytes: z.number().int().nonnegative(),
	used_bytes: z.number().int().nonnegative(),
	remaining_bytes: z.number().int().nonnegative(),
});

export type AdminUserQuotaSummaryItem = z.infer<
	typeof AdminUserQuotaSummaryItemSchema
>;

export const AdminUserQuotaSummariesResponseSchema = z.object({
	partial: z.boolean(),
	unreachable_nodes: z.array(z.string()),
	items: z.array(AdminUserQuotaSummaryItemSchema),
});

export type AdminUserQuotaSummariesResponse = z.infer<
	typeof AdminUserQuotaSummariesResponseSchema
>;

export async function fetchAdminUserQuotaSummaries(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminUserQuotaSummariesResponse> {
	return fetchJsonWithTimeout(
		"/api/admin/users/quota-summaries",
		{
			method: "GET",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
		(value) => AdminUserQuotaSummariesResponseSchema.parse(value),
	);
}
