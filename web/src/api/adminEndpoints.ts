import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminEndpointKindSchema = z.enum([
	"vless_reality_vision_tcp",
	"ss2022_2022_blake3_aes_128_gcm",
]);

export type AdminEndpointKind = z.infer<typeof AdminEndpointKindSchema>;

export const RealityConfigSchema = z.object({
	dest: z.string(),
	server_names: z.array(z.string()),
	fingerprint: z.string(),
});

export type RealityConfig = z.infer<typeof RealityConfigSchema>;

export const AdminEndpointSchema = z.object({
	endpoint_id: z.string(),
	node_id: z.string(),
	tag: z.string(),
	kind: AdminEndpointKindSchema,
	port: z.number().int().nonnegative(),
	meta: z.record(z.unknown()),
});

export type AdminEndpoint = z.infer<typeof AdminEndpointSchema>;

export const AdminEndpointsResponseSchema = z.object({
	items: z.array(AdminEndpointSchema),
});

export type AdminEndpointsResponse = z.infer<
	typeof AdminEndpointsResponseSchema
>;

export const AdminEndpointRotateResponseSchema = z.object({
	endpoint_id: z.string(),
	active_short_id: z.string(),
	short_ids: z.array(z.string()),
});

export type AdminEndpointRotateResponse = z.infer<
	typeof AdminEndpointRotateResponseSchema
>;

export type AdminEndpointCreateRequest =
	| {
			kind: "vless_reality_vision_tcp";
			node_id: string;
			port: number;
			public_domain: string;
			reality: RealityConfig;
	  }
	| {
			kind: "ss2022_2022_blake3_aes_128_gcm";
			node_id: string;
			port: number;
	  };

export type AdminEndpointPatchRequest = {
	port?: number;
	public_domain?: string;
	reality?: RealityConfig;
};

export async function fetchAdminEndpoints(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminEndpointsResponse> {
	const res = await fetch("/api/admin/endpoints", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminEndpointsResponseSchema.parse(json);
}

export async function fetchAdminEndpoint(
	adminToken: string,
	endpointId: string,
	signal?: AbortSignal,
): Promise<AdminEndpoint> {
	const res = await fetch(`/api/admin/endpoints/${endpointId}`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminEndpointSchema.parse(json);
}

export async function createAdminEndpoint(
	adminToken: string,
	payload: AdminEndpointCreateRequest,
	signal?: AbortSignal,
): Promise<AdminEndpoint> {
	const res = await fetch("/api/admin/endpoints", {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify(payload),
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminEndpointSchema.parse(json);
}

export async function patchAdminEndpoint(
	adminToken: string,
	endpointId: string,
	payload: AdminEndpointPatchRequest,
	signal?: AbortSignal,
): Promise<AdminEndpoint> {
	const res = await fetch(`/api/admin/endpoints/${endpointId}`, {
		method: "PATCH",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify(payload),
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminEndpointSchema.parse(json);
}

export async function deleteAdminEndpoint(
	adminToken: string,
	endpointId: string,
	signal?: AbortSignal,
): Promise<void> {
	const res = await fetch(`/api/admin/endpoints/${endpointId}`, {
		method: "DELETE",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);
}

export async function rotateAdminEndpointShortId(
	adminToken: string,
	endpointId: string,
	signal?: AbortSignal,
): Promise<AdminEndpointRotateResponse> {
	const res = await fetch(`/api/admin/endpoints/${endpointId}/rotate-shortid`, {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminEndpointRotateResponseSchema.parse(json);
}
