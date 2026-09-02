import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AlertItemSchema = z.object({
	type: z.string(),
	membership_key: z.string(),
	user_id: z.string(),
	endpoint_id: z.string(),
	owner_node_id: z.string(),
	quota_banned: z.boolean(),
	quota_banned_at: z.string().nullable(),
	message: z.string(),
	action_hint: z.string(),
	node_id: z.string().optional(),
	resource_node_id: z.string().optional(),
	scope: z.string().optional(),
	metric: z.string().optional(),
	severity: z.string().optional(),
	opened_at: z.string().optional(),
	latest_bucket_start_unix_seconds: z.number().optional(),
});

export type AlertItem = z.infer<typeof AlertItemSchema>;

export const AlertsResponseSchema = z
	.object({
		partial: z.boolean(),
		unreachable_nodes: z.array(z.string()),
		items: z.array(AlertItemSchema),
	})
	.passthrough();

export type AlertsResponse = z.infer<typeof AlertsResponseSchema>;

export async function fetchAdminAlerts(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AlertsResponse> {
	const res = await fetch("/api/admin/alerts", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AlertsResponseSchema.parse(json);
}
