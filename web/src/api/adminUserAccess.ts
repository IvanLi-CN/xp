import { z } from "zod";

import { throwIfNotOk } from "./backendError";
import { GrantCredentialsSchema } from "./grantCredentials";

export const AdminUserAccessMembershipSchema = z.object({
	user_id: z.string(),
	node_id: z.string(),
	endpoint_id: z.string(),
});

export type AdminUserAccessMembership = z.infer<
	typeof AdminUserAccessMembershipSchema
>;

export const AdminUserAccessGrantSchema = z.object({
	grant_id: z.string(),
	enabled: z.boolean(),
	quota_limit_bytes: z.number().int().nonnegative(),
	note: z.string().nullable(),
	credentials: GrantCredentialsSchema,
});

export type AdminUserAccessGrant = z.infer<typeof AdminUserAccessGrantSchema>;

export const AdminUserAccessItemSchema = z.object({
	membership: AdminUserAccessMembershipSchema,
	grant: AdminUserAccessGrantSchema,
});

export type AdminUserAccessItem = z.infer<typeof AdminUserAccessItemSchema>;

export const AdminUserAccessResponseSchema = z.object({
	items: z.array(AdminUserAccessItemSchema),
});

export type AdminUserAccessResponse = z.infer<
	typeof AdminUserAccessResponseSchema
>;

export type AdminUserAccessReplaceRequest = {
	items: Array<{
		endpoint_id: string;
		note?: string | null;
	}>;
};

export async function fetchAdminUserAccess(
	adminToken: string,
	userId: string,
	signal?: AbortSignal,
): Promise<AdminUserAccessResponse> {
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
	return AdminUserAccessResponseSchema.parse(json);
}

export async function replaceAdminUserAccess(
	adminToken: string,
	userId: string,
	payload: AdminUserAccessReplaceRequest,
	signal?: AbortSignal,
): Promise<AdminUserAccessResponse> {
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
	return AdminUserAccessResponseSchema.parse(json);
}
