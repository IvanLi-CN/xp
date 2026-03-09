import { z } from "zod";

import { AdminNodeSchema } from "./adminNodes";
import { throwIfNotOk } from "./backendError";

export const GeoDbProviderSchema = z.enum(["dbip_lite"]);
export const GeoDbLocalModeSchema = z.enum([
	"managed",
	"external_override",
	"missing",
]);

export const GeoDbUpdateSettingsSchema = z.object({
	provider: GeoDbProviderSchema,
	auto_update_enabled: z.boolean(),
	update_interval_days: z.number().int().min(1).max(30),
});

const OptionalDateTimeSchema = z
	.string()
	.nullish()
	.transform((value) => value ?? null);
const OptionalStringSchema = z
	.string()
	.nullish()
	.transform((value) => value ?? null);

export const AdminIpGeoDbNodeStatusSchema = z.object({
	node: AdminNodeSchema,
	mode: GeoDbLocalModeSchema,
	running: z.boolean(),
	city_db_path: z.string(),
	asn_db_path: z.string(),
	last_started_at: OptionalDateTimeSchema,
	last_success_at: OptionalDateTimeSchema,
	next_scheduled_at: OptionalDateTimeSchema,
	last_error: OptionalStringSchema,
});

export const AdminIpGeoDbResponseSchema = z.object({
	settings: GeoDbUpdateSettingsSchema,
	partial: z.boolean(),
	unreachable_nodes: z.array(z.string()),
	nodes: z.array(AdminIpGeoDbNodeStatusSchema),
});

export const AdminIpGeoDbUpdateNodeResultSchema = z.object({
	node_id: z.string(),
	status: z.enum(["accepted", "already_running", "skipped", "error"]),
	message: OptionalStringSchema,
});

export const AdminIpGeoDbUpdateResponseSchema = z.object({
	partial: z.boolean(),
	unreachable_nodes: z.array(z.string()),
	nodes: z.array(AdminIpGeoDbUpdateNodeResultSchema),
});

export type GeoDbUpdateSettings = z.infer<typeof GeoDbUpdateSettingsSchema>;
export type GeoDbLocalMode = z.infer<typeof GeoDbLocalModeSchema>;
export type AdminIpGeoDbNodeStatus = z.infer<
	typeof AdminIpGeoDbNodeStatusSchema
>;
export type AdminIpGeoDbResponse = z.infer<typeof AdminIpGeoDbResponseSchema>;
export type AdminIpGeoDbUpdateNodeResult = z.infer<
	typeof AdminIpGeoDbUpdateNodeResultSchema
>;
export type AdminIpGeoDbUpdateResponse = z.infer<
	typeof AdminIpGeoDbUpdateResponseSchema
>;

export async function fetchAdminIpGeoDb(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminIpGeoDbResponse> {
	const res = await fetch("/api/admin/ip-geo-db", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);
	return AdminIpGeoDbResponseSchema.parse(await res.json());
}

export async function patchAdminIpGeoDb(
	adminToken: string,
	input: Pick<
		GeoDbUpdateSettings,
		"auto_update_enabled" | "update_interval_days"
	>,
): Promise<GeoDbUpdateSettings> {
	const res = await fetch("/api/admin/ip-geo-db", {
		method: "PATCH",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify(input),
	});

	await throwIfNotOk(res);
	return GeoDbUpdateSettingsSchema.parse(await res.json());
}

export async function triggerAdminIpGeoDbUpdate(
	adminToken: string,
): Promise<AdminIpGeoDbUpdateResponse> {
	const res = await fetch("/api/admin/ip-geo-db/update", {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
	});

	await throwIfNotOk(res);
	return AdminIpGeoDbUpdateResponseSchema.parse(await res.json());
}
