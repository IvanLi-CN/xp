import { z } from "zod";

import { AdminNodeSchema } from "./adminNodes";
import { fetchJsonWithTimeout } from "./request";

export const AdminTcpConnectionUsageWindowSchema = z.enum([
	"24h",
	"7d",
	"30d",
	"90d",
]);

export const AdminTcpConnectionHistoryWindowSchema =
	AdminTcpConnectionUsageWindowSchema;

export const AdminTcpConnectionUsageWarningSchema = z.object({
	code: z.string(),
	message: z.string(),
});

export const AdminTcpConnectionEndpointOptionSchema = z.object({
	endpoint_id: z.string(),
	endpoint_tag: z.string(),
	port: z.number().int().nonnegative(),
});

export const AdminTcpConnectionSeriesPointSchema = z.object({
	minute: z.string(),
	count: z.number().int().nonnegative(),
});

export const AdminTcpConnectionEndpointSeriesSchema = z.object({
	endpoint_id: z.string(),
	endpoint_tag: z.string(),
	port: z.number().int().nonnegative(),
	series: z.array(AdminTcpConnectionSeriesPointSchema),
});

export const AdminTcpConnectionHistoryPointSchema = z.object({
	bucket_start: z.string(),
	min: z.number().int().nonnegative(),
	avg: z.number().nonnegative(),
	max: z.number().int().nonnegative(),
	samples_expected: z.number().int().nonnegative(),
	samples_actual: z.number().int().nonnegative(),
	truncated: z.boolean(),
	partial: z.boolean(),
});

export const AdminTcpConnectionHistorySeriesSchema = z.object({
	approximate: z.boolean(),
	points: z.array(AdminTcpConnectionHistoryPointSchema),
});

export const AdminTcpConnectionHistoryClassSchema = z.object({
	class: z.string(),
	approximate: z.boolean(),
	points: z.array(AdminTcpConnectionHistoryPointSchema),
});

export const AdminTcpConnectionHistorySubjectSchema = z.object({
	class: z.string(),
	subject: z.string(),
	approximate: z.boolean(),
	points: z.array(AdminTcpConnectionHistoryPointSchema),
});

export const AdminTcpConnectionDeletedSubjectSchema = z.object({
	class: z.string(),
	subject: z.string(),
});

export const AdminTcpConnectionGapSchema = z.object({
	start: z.string(),
	end: z.string(),
	reason: z.string(),
});

export const AdminTcpConnectionPartialBucketSchema = z.object({
	bucket_start: z.string(),
	reason: z.string(),
});

export const AdminTcpConnectionHistorySchema = z.object({
	window: AdminTcpConnectionHistoryWindowSchema,
	resolution: z.enum(["1m", "5m", "daily"]),
	window_start: z.string(),
	window_end: z.string(),
	quality: z.enum(["complete", "partial", "unsupported"]),
	reasons: z.array(z.string()),
	truncated: z.boolean(),
	truncated_buckets: z.array(z.string()),
	partial_buckets: z.array(AdminTcpConnectionPartialBucketSchema),
	deleted_subjects: z.array(AdminTcpConnectionDeletedSubjectSchema),
	subjects_truncated: z.boolean(),
	subjects_omitted: z.boolean(),
	subjects_next_cursor: z.string().nullable(),
	watermark: z.string().nullable().optional(),
	gaps: z.array(AdminTcpConnectionGapSchema),
	total: AdminTcpConnectionHistorySeriesSchema,
	classes: z.array(AdminTcpConnectionHistoryClassSchema),
	subjects: z.array(AdminTcpConnectionHistorySubjectSchema),
});

/**
 * Additive contract: older nodes send only the legacy inbound fields, so every full-site field
 * defaults instead of failing the parse. `hasFullSiteHistory` distinguishes the two shapes.
 */
export const AdminNodeTcpConnectionsResponseSchema = z.object({
	node: AdminNodeSchema,
	window: AdminTcpConnectionHistoryWindowSchema,
	window_start: z.string(),
	window_end: z.string(),
	warnings: z.array(AdminTcpConnectionUsageWarningSchema),
	endpoints: z.array(AdminTcpConnectionEndpointOptionSchema),
	per_endpoint_series: z.array(AdminTcpConnectionEndpointSeriesSchema),
	quality: z.enum(["complete", "partial", "unsupported"]).optional(),
	reasons: z.array(z.string()).optional(),
	truncated: z.boolean().optional(),
	truncated_buckets: z.array(z.string()).optional(),
	partial_buckets: z.array(AdminTcpConnectionPartialBucketSchema).optional(),
	deleted_subjects: z.array(AdminTcpConnectionDeletedSubjectSchema).optional(),
	subjects_truncated: z.boolean().optional(),
	subjects_omitted: z.boolean().optional(),
	subjects_next_cursor: z.string().nullable().optional(),
	resolution: z.enum(["1m", "5m", "daily"]).optional(),
	watermark: z.string().nullable().optional(),
	gaps: z.array(AdminTcpConnectionGapSchema).optional(),
	total: AdminTcpConnectionHistorySeriesSchema.optional(),
	classes: z.array(AdminTcpConnectionHistoryClassSchema).optional(),
	subjects: z.array(AdminTcpConnectionHistorySubjectSchema).optional(),
});

/** Older nodes omit the full-site fields entirely; treat that as the legacy inbound view. */
export function hasFullSiteHistory(
	response: Pick<AdminNodeTcpConnectionsResponse, "total">,
): boolean {
	return (response.total?.points.length ?? 0) > 0;
}

export type AdminTcpConnectionUsageWindow = z.infer<
	typeof AdminTcpConnectionUsageWindowSchema
>;
export type AdminTcpConnectionHistoryWindow = AdminTcpConnectionUsageWindow;
export type AdminTcpConnectionUsageWarning = z.infer<
	typeof AdminTcpConnectionUsageWarningSchema
>;
export type AdminTcpConnectionEndpointOption = z.infer<
	typeof AdminTcpConnectionEndpointOptionSchema
>;
export type AdminTcpConnectionSeriesPoint = z.infer<
	typeof AdminTcpConnectionSeriesPointSchema
>;
export type AdminTcpConnectionEndpointSeries = z.infer<
	typeof AdminTcpConnectionEndpointSeriesSchema
>;
export type AdminTcpConnectionHistoryPoint = z.infer<
	typeof AdminTcpConnectionHistoryPointSchema
>;
export type AdminTcpConnectionHistorySeries = z.infer<
	typeof AdminTcpConnectionHistorySeriesSchema
>;
export type AdminTcpConnectionHistoryClass = z.infer<
	typeof AdminTcpConnectionHistoryClassSchema
>;
export type AdminTcpConnectionHistorySubject = z.infer<
	typeof AdminTcpConnectionHistorySubjectSchema
>;
export type AdminTcpConnectionDeletedSubject = z.infer<
	typeof AdminTcpConnectionDeletedSubjectSchema
>;
export type AdminNodeTcpConnectionsResponse = z.infer<
	typeof AdminNodeTcpConnectionsResponseSchema
>;

export async function fetchAdminNodeTcpConnections(
	adminToken: string,
	nodeId: string,
	window: AdminTcpConnectionHistoryWindow,
	signal?: AbortSignal,
): Promise<AdminNodeTcpConnectionsResponse> {
	return fetchJsonWithTimeout(
		`/api/admin/nodes/${encodeURIComponent(nodeId)}/tcp-connections?window=${window}`,
		{
			method: "GET",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
		(value) => AdminNodeTcpConnectionsResponseSchema.parse(value),
	);
}
