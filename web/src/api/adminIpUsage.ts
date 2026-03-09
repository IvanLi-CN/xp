import { z } from "zod";

import { AdminNodeSchema } from "./adminNodes";
import { throwIfNotOk } from "./backendError";

export const AdminIpUsageWindowSchema = z.enum(["24h", "7d"]);

export const AdminIpUsageWarningSchema = z.object({
	code: z.string(),
	message: z.string(),
});

export const AdminIpGeoSourceSchema = z.enum([
	"managed_dbip_lite",
	"external_override",
	"missing",
]);

export const AdminIpUsageSeriesPointSchema = z.object({
	minute: z.string(),
	count: z.number().int().nonnegative(),
});

export const AdminIpUsageTimelineSegmentSchema = z.object({
	start_minute: z.string(),
	end_minute: z.string(),
});

export const AdminIpUsageTimelineLaneSchema = z.object({
	lane_key: z.string(),
	endpoint_id: z.string(),
	endpoint_tag: z.string(),
	ip: z.string(),
	minutes: z.number().int().nonnegative(),
	segments: z.array(AdminIpUsageTimelineSegmentSchema),
});

export const AdminIpUsageListEntrySchema = z.object({
	ip: z.string(),
	minutes: z.number().int().nonnegative(),
	endpoint_tags: z.array(z.string()),
	region: z.string(),
	operator: z.string(),
	last_seen_at: z.string(),
});

export const AdminNodeIpUsageResponseSchema = z.object({
	node: AdminNodeSchema,
	window: AdminIpUsageWindowSchema,
	geo_source: AdminIpGeoSourceSchema,
	window_start: z.string(),
	window_end: z.string(),
	warnings: z.array(AdminIpUsageWarningSchema),
	unique_ip_series: z.array(AdminIpUsageSeriesPointSchema),
	timeline: z.array(AdminIpUsageTimelineLaneSchema),
	ips: z.array(AdminIpUsageListEntrySchema),
});

export const AdminUserIpUsageNodeGroupSchema = z.object({
	node: AdminNodeSchema,
	geo_source: AdminIpGeoSourceSchema,
	window_start: z.string(),
	window_end: z.string(),
	warnings: z.array(AdminIpUsageWarningSchema),
	unique_ip_series: z.array(AdminIpUsageSeriesPointSchema),
	timeline: z.array(AdminIpUsageTimelineLaneSchema),
	ips: z.array(AdminIpUsageListEntrySchema),
});

export const AdminUserIpUsageResponseSchema = z.object({
	user: z.object({
		user_id: z.string(),
		display_name: z.string(),
	}),
	window: AdminIpUsageWindowSchema,
	partial: z.boolean(),
	unreachable_nodes: z.array(z.string()),
	warnings: z.array(AdminIpUsageWarningSchema),
	groups: z.array(AdminUserIpUsageNodeGroupSchema),
});

export type AdminIpUsageWindow = z.infer<typeof AdminIpUsageWindowSchema>;
export type AdminIpUsageWarning = z.infer<typeof AdminIpUsageWarningSchema>;
export type AdminIpGeoSource = z.infer<typeof AdminIpGeoSourceSchema>;
export type AdminIpUsageSeriesPoint = z.infer<
	typeof AdminIpUsageSeriesPointSchema
>;
export type AdminIpUsageTimelineSegment = z.infer<
	typeof AdminIpUsageTimelineSegmentSchema
>;
export type AdminIpUsageTimelineLane = z.infer<
	typeof AdminIpUsageTimelineLaneSchema
>;
export type AdminIpUsageListEntry = z.infer<typeof AdminIpUsageListEntrySchema>;
export type AdminNodeIpUsageResponse = z.infer<
	typeof AdminNodeIpUsageResponseSchema
>;
export type AdminUserIpUsageNodeGroup = z.infer<
	typeof AdminUserIpUsageNodeGroupSchema
>;
export type AdminUserIpUsageResponse = z.infer<
	typeof AdminUserIpUsageResponseSchema
>;

export async function fetchAdminNodeIpUsage(
	adminToken: string,
	nodeId: string,
	window: AdminIpUsageWindow,
	signal?: AbortSignal,
): Promise<AdminNodeIpUsageResponse> {
	const res = await fetch(
		`/api/admin/nodes/${encodeURIComponent(nodeId)}/ip-usage?window=${window}`,
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
	const json: unknown = await res.json();
	return AdminNodeIpUsageResponseSchema.parse(json);
}

export async function fetchAdminUserIpUsage(
	adminToken: string,
	userId: string,
	window: AdminIpUsageWindow,
	signal?: AbortSignal,
): Promise<AdminUserIpUsageResponse> {
	const res = await fetch(
		`/api/admin/users/${encodeURIComponent(userId)}/ip-usage?window=${window}`,
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
	const json: unknown = await res.json();
	return AdminUserIpUsageResponseSchema.parse(json);
}
