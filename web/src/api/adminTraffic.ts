import { z } from "zod";

import { AdminNodeSchema } from "./adminNodes";
import { throwIfNotOk } from "./backendError";

export const TrafficWindowSchema = z.enum(["24h", "31d"]);
export type TrafficWindow = z.infer<typeof TrafficWindowSchema>;

export const TrafficSeriesPointSchema = z.object({
	start_at: z.string(),
	end_at: z.string(),
	uplink_bytes: z.number().int().nonnegative().nullable().optional(),
	downlink_bytes: z.number().int().nonnegative().nullable().optional(),
	total_bytes: z.number().int().nonnegative().nullable().optional(),
	complete: z.boolean(),
	is_current_day: z.boolean(),
});

export const TrafficSummarySchema = z.object({
	mode: z.string(),
	cycle_start_at: z.string().nullable().optional(),
	cycle_end_at: z.string().nullable().optional(),
	uplink_bytes: z.number().int().nonnegative(),
	downlink_bytes: z.number().int().nonnegative(),
	total_bytes: z.number().int().nonnegative(),
	complete: z.boolean(),
	tracking_since: z.string().nullable().optional(),
});

export const TrafficReportSchema = z.object({
	window: TrafficWindowSchema,
	window_start_at: z.string(),
	window_end_at: z.string(),
	timezone: z.string(),
	summary: TrafficSummarySchema,
	current: z.array(TrafficSeriesPointSchema),
	reference: z.array(TrafficSeriesPointSchema).nullable().optional(),
	partial: z.boolean(),
	last_sample_at: z.string().nullable().optional(),
	warnings: z.array(z.string()),
});

export type TrafficSeriesPoint = z.infer<typeof TrafficSeriesPointSchema>;
export type TrafficSummary = z.infer<typeof TrafficSummarySchema>;
export type TrafficReport = z.infer<typeof TrafficReportSchema>;

export const AdminNodeTrafficResponseSchema = z.object({
	node: AdminNodeSchema,
	traffic: TrafficReportSchema,
});
export type AdminNodeTrafficResponse = z.infer<
	typeof AdminNodeTrafficResponseSchema
>;

export const UserTrafficNodeOptionSchema = z.object({
	node_id: z.string(),
	node_name: z.string(),
});
export type UserTrafficNodeOption = z.infer<typeof UserTrafficNodeOptionSchema>;

export const AdminUserTrafficResponseSchema = z.object({
	user: z.object({ user_id: z.string(), display_name: z.string() }),
	traffic: TrafficReportSchema,
	nodes: z.array(UserTrafficNodeOptionSchema),
	partial: z.boolean(),
	unreachable_nodes: z.array(z.string()),
});
export type AdminUserTrafficResponse = z.infer<
	typeof AdminUserTrafficResponseSchema
>;

export async function fetchAdminNodeTraffic(
	adminToken: string,
	nodeId: string,
	window: TrafficWindow,
	signal?: AbortSignal,
): Promise<AdminNodeTrafficResponse> {
	const res = await fetch(
		`/api/admin/nodes/${encodeURIComponent(nodeId)}/traffic?window=${window}`,
		{
			method: "GET",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
	);
	await throwIfNotOk(res);
	return AdminNodeTrafficResponseSchema.parse(await res.json());
}

export async function fetchAdminUserTraffic(
	adminToken: string,
	userId: string,
	window: TrafficWindow,
	nodeId?: string | null,
	signal?: AbortSignal,
): Promise<AdminUserTrafficResponse> {
	const params = new URLSearchParams({ window });
	if (nodeId) params.set("node_id", nodeId);
	const res = await fetch(
		`/api/admin/users/${encodeURIComponent(userId)}/traffic?${params.toString()}`,
		{
			method: "GET",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
	);
	await throwIfNotOk(res);
	return AdminUserTrafficResponseSchema.parse(await res.json());
}
