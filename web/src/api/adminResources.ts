import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const ResourceCapabilitySchema = z.enum([
	"supported",
	"partial",
	"unsupported",
]);
export const ResourceDomainSchema = z.enum(["host", "cgroup"]);
export const ResourceRoleSchema = z.enum([
	"xp",
	"xray",
	"cloudflared",
	"canary",
]);

const MeasurementSchema = z.object({
	capability: ResourceCapabilitySchema,
	value: z.number().optional(),
	reason_code: z.string().optional(),
});

const FilesystemSchema = z.object({
	mount: z.string(),
	capability: ResourceCapabilitySchema,
	total_bytes: z.number().optional(),
	available_bytes: z.number().optional(),
	used_percent: z.number().optional(),
	total_inodes: z.number().optional(),
	available_inodes: z.number().optional(),
	used_inode_percent: z.number().optional(),
	reason_code: z.string().optional(),
});

export const ResourceSnapshotSchema = z.object({
	node_id: z.string(),
	observed_at: z.string(),
	resource_domain: ResourceDomainSchema,
	capture_state: z.string(),
	capability: ResourceCapabilitySchema,
	domain: z.object({
		cpu_busy_percent: MeasurementSchema,
		cpu_iowait_percent: MeasurementSchema,
		load1: MeasurementSchema,
		memory_total_bytes: MeasurementSchema,
		memory_available_bytes: MeasurementSchema,
		swap_total_bytes: MeasurementSchema,
		swap_free_bytes: MeasurementSchema,
		filesystems: z.array(FilesystemSchema),
	}),
	runtimes: z.array(
		z.object({
			role: ResourceRoleSchema,
			state: z.string(),
			capability: ResourceCapabilitySchema,
			metrics: z.object({
				cpu_percent: MeasurementSchema,
				rss_bytes: MeasurementSchema,
				pss_bytes: MeasurementSchema,
				read_bytes_per_second: MeasurementSchema,
				write_bytes_per_second: MeasurementSchema,
				fd_count: MeasurementSchema,
				thread_count: MeasurementSchema,
			}),
		}),
	),
});

export type ResourceSnapshot = z.infer<typeof ResourceSnapshotSchema>;

export const ResourceSeriesPointSchema = z.object({
	observed_at: z.string(),
	value: z.number().nullable().optional(),
	capability: ResourceCapabilitySchema,
});
export const ResourceRecentSeriesSchema = z.object({
	metric: z.string(),
	role: ResourceRoleSchema.nullable().optional(),
	resolution: z.string(),
	points: z.array(ResourceSeriesPointSchema),
	truncated: z.boolean(),
});
export type ResourceRecentSeries = z.infer<typeof ResourceRecentSeriesSchema>;

export const ResourceHistoryResponseSchema = z.object({
	metric: z.string(),
	role: ResourceRoleSchema.nullable().optional(),
	resolution: z.string(),
	quality: z.string(),
	coverage: z.tuple([z.number(), z.number()]).nullable().optional(),
	watermark: z.number().nullable().optional(),
	gaps: z.array(
		z.object({
			from_bucket_start_unix_seconds: z.number(),
			to_bucket_start_unix_seconds: z.number(),
			reason_code: z.string(),
		}),
	),
	freshness_seconds: z.number().nullable().optional(),
	truncated: z.boolean(),
	points: z.array(ResourceSeriesPointSchema),
});
export type ResourceHistoryResponse = z.infer<
	typeof ResourceHistoryResponseSchema
>;

export const AdminNodesResourcesResponseSchema = z.object({
	partial: z.boolean(),
	unreachable_nodes: z.array(z.string()),
	items: z.array(ResourceSnapshotSchema),
});
export type AdminNodesResourcesResponse = z.infer<
	typeof AdminNodesResourcesResponseSchema
>;

export const ResourcePolicyOverrideSchema = z.object({
	enabled: z.boolean().optional(),
	cpu_warning_percent: z.number().optional(),
	cpu_warning_minutes: z.number().optional(),
	cpu_critical_percent: z.number().optional(),
	cpu_critical_minutes: z.number().optional(),
	memory_warning_percent: z.number().optional(),
	memory_warning_minutes: z.number().optional(),
	memory_critical_percent: z.number().optional(),
	memory_critical_minutes: z.number().optional(),
	disk_warning_percent: z.number().optional(),
	disk_critical_percent: z.number().optional(),
});

export const ResourcePolicySchema = z.object({
	revision: z.number(),
	enabled: z.boolean(),
	cpu_warning_percent: z.number(),
	cpu_warning_minutes: z.number(),
	cpu_critical_percent: z.number(),
	cpu_critical_minutes: z.number(),
	memory_warning_percent: z.number(),
	memory_warning_minutes: z.number(),
	memory_critical_percent: z.number(),
	memory_critical_minutes: z.number(),
	disk_warning_percent: z.number(),
	disk_critical_percent: z.number(),
	node_overrides: z
		.record(z.string(), ResourcePolicyOverrideSchema)
		.default({}),
	role_overrides: z
		.record(ResourceRoleSchema, ResourcePolicyOverrideSchema)
		.optional()
		.transform((value) => value ?? {}),
});
export type ResourcePolicy = z.infer<typeof ResourcePolicySchema>;

async function getJson<T>(
	path: string,
	adminToken: string,
	schema: z.ZodType<T>,
	signal?: AbortSignal,
): Promise<T> {
	const res = await fetch(path, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});
	await throwIfNotOk(res);
	return schema.parse(await res.json());
}

export function fetchAdminNodesResources(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminNodesResourcesResponse> {
	return getJson(
		"/api/admin/nodes/resources",
		adminToken,
		AdminNodesResourcesResponseSchema,
		signal,
	);
}

export function fetchAdminNodeResources(
	adminToken: string,
	nodeId: string,
	signal?: AbortSignal,
): Promise<ResourceSnapshot> {
	return getJson(
		`/api/admin/nodes/${nodeId}/resources`,
		adminToken,
		ResourceSnapshotSchema,
		signal,
	);
}

export function fetchAdminNodeResourceRecent(
	adminToken: string,
	nodeId: string,
	metric: string,
	role?: string,
	signal?: AbortSignal,
): Promise<ResourceRecentSeries> {
	const params = new URLSearchParams({ metric });
	if (role) params.set("role", role);
	return getJson(
		`/api/admin/nodes/${nodeId}/resources/recent?${params.toString()}`,
		adminToken,
		ResourceRecentSeriesSchema,
		signal,
	);
}

export function fetchAdminNodeResourceHistory(
	adminToken: string,
	nodeId: string,
	metric: string,
	signal?: AbortSignal,
): Promise<ResourceHistoryResponse> {
	const params = new URLSearchParams({
		metric,
		resolution: "auto",
		limit: "1500",
	});
	return getJson(
		`/api/admin/nodes/${nodeId}/resources/history?${params.toString()}`,
		adminToken,
		ResourceHistoryResponseSchema,
		signal,
	);
}

export function fetchAdminResourcePolicy(
	adminToken: string,
	signal?: AbortSignal,
): Promise<ResourcePolicy> {
	return getJson(
		"/api/admin/resource-monitoring/policy",
		adminToken,
		ResourcePolicySchema,
		signal,
	);
}
