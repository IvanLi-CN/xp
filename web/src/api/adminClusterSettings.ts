import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminClusterSettingsResponseSchema = z.object({
	ip_geo_enabled: z.boolean(),
	ip_geo_origin: z.string(),
	legacy_fallback_in_use: z.boolean(),
});

export type AdminClusterSettingsResponse = z.infer<
	typeof AdminClusterSettingsResponseSchema
>;

export type PutAdminClusterSettingsRequest = {
	ip_geo_enabled: boolean;
	ip_geo_origin: string;
};

export async function fetchAdminClusterSettings(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminClusterSettingsResponse> {
	const res = await fetch("/api/admin/cluster-settings", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminClusterSettingsResponseSchema.parse(json);
}

export async function putAdminClusterSettings(
	adminToken: string,
	input: PutAdminClusterSettingsRequest,
): Promise<AdminClusterSettingsResponse> {
	const res = await fetch("/api/admin/cluster-settings", {
		method: "PUT",
		headers: {
			Accept: "application/json",
			"Content-Type": "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		body: JSON.stringify(input),
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminClusterSettingsResponseSchema.parse(json);
}
