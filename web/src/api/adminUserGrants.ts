import { z } from "zod";

import { throwIfNotOk } from "./backendError";
import { GrantCredentialsSchema } from "./grantCredentials";

export const AdminUserGrantSchema = z.object({
	grant_id: z.string(),
	user_id: z.string(),
	endpoint_id: z.string(),
	enabled: z.boolean(),
	quota_limit_bytes: z.number().int().nonnegative(),
	note: z.string().nullable().optional(),
	credentials: GrantCredentialsSchema,
});

export type AdminUserGrant = z.infer<typeof AdminUserGrantSchema>;

export const AdminUserGrantsResponseSchema = z.object({
	items: z.array(AdminUserGrantSchema),
});

export type AdminUserGrantsResponse = z.infer<
	typeof AdminUserGrantsResponseSchema
>;

export type PutAdminUserGrantItem = {
	endpoint_id: string;
	enabled: boolean;
	quota_limit_bytes: number;
	note?: string | null;
};

export type PutAdminUserGrantsRequest = {
	items: PutAdminUserGrantItem[];
};

export const PutAdminUserGrantsResponseSchema = z.object({
	created: z.number().int().nonnegative(),
	updated: z.number().int().nonnegative(),
	deleted: z.number().int().nonnegative(),
	items: z.array(AdminUserGrantSchema),
});

export type PutAdminUserGrantsResponse = z.infer<
	typeof PutAdminUserGrantsResponseSchema
>;

export async function fetchAdminUserGrants(
	adminToken: string,
	userId: string,
	signal?: AbortSignal,
): Promise<AdminUserGrantsResponse> {
	const res = await fetch(`/api/admin/users/${userId}/grants`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUserGrantsResponseSchema.parse(json);
}

export async function putAdminUserGrants(
	adminToken: string,
	userId: string,
	payload: PutAdminUserGrantsRequest,
	signal?: AbortSignal,
): Promise<PutAdminUserGrantsResponse> {
	const res = await fetch(`/api/admin/users/${userId}/grants`, {
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
	return PutAdminUserGrantsResponseSchema.parse(json);
}
