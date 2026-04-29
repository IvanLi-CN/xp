import { z } from "zod";

import { AdminEndpointKindSchema } from "./adminEndpoints";
import { throwIfNotOk } from "./backendError";

export const AdminUserAccessItemSchema = z.object({
	user_id: z.string(),
	endpoint_id: z.string(),
	node_id: z.string(),
});

export type AdminUserAccessItem = z.infer<typeof AdminUserAccessItemSchema>;

export const GetAdminUserAccessResponseSchema = z.object({
	items: z.array(AdminUserAccessItemSchema),
	auto_assign_endpoint_kinds: z.array(AdminEndpointKindSchema),
});

export type GetAdminUserAccessResponse = z.infer<
	typeof GetAdminUserAccessResponseSchema
>;

export type PutAdminUserAccessRequest = {
	items: Array<{ endpoint_id: string }>;
};

export const PutAdminUserAccessResponseSchema = z.object({
	created: z.number().int().nonnegative(),
	deleted: z.number().int().nonnegative(),
	items: z.array(AdminUserAccessItemSchema),
	auto_assign_endpoint_kinds: z.array(AdminEndpointKindSchema),
});

export type PutAdminUserAccessResponse = z.infer<
	typeof PutAdminUserAccessResponseSchema
>;

export async function fetchAdminUserAccess(
	adminToken: string,
	userId: string,
	signal?: AbortSignal,
): Promise<GetAdminUserAccessResponse> {
	const res = await fetch(`/api/admin/users/${userId}/access`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return GetAdminUserAccessResponseSchema.parse(json);
}

export async function putAdminUserAccess(
	adminToken: string,
	userId: string,
	payload: PutAdminUserAccessRequest,
	signal?: AbortSignal,
): Promise<PutAdminUserAccessResponse> {
	const res = await fetch(`/api/admin/users/${userId}/access`, {
		method: "PUT",
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
	return PutAdminUserAccessResponseSchema.parse(json);
}
