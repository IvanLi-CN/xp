import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const CyclePolicyDefaultSchema = z.enum(["by_user", "by_node"]);

export type CyclePolicyDefault = z.infer<typeof CyclePolicyDefaultSchema>;

export const AdminUserSchema = z.object({
	user_id: z.string(),
	display_name: z.string(),
	subscription_token: z.string(),
	cycle_policy_default: CyclePolicyDefaultSchema,
	cycle_day_of_month_default: z.number().int().min(1).max(31),
});

export type AdminUser = z.infer<typeof AdminUserSchema>;

export const AdminUsersResponseSchema = z.object({
	items: z.array(AdminUserSchema),
});

export type AdminUsersResponse = z.infer<typeof AdminUsersResponseSchema>;

export const AdminUserTokenResponseSchema = z.object({
	subscription_token: z.string(),
});

export type AdminUserTokenResponse = z.infer<
	typeof AdminUserTokenResponseSchema
>;

export type AdminUserCreateRequest = {
	display_name: string;
	cycle_policy_default: CyclePolicyDefault;
	cycle_day_of_month_default: number;
};

export type AdminUserPatchRequest = {
	display_name?: string;
	cycle_policy_default?: CyclePolicyDefault;
	cycle_day_of_month_default?: number;
};

export async function fetchAdminUsers(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminUsersResponse> {
	const res = await fetch("/api/admin/users", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUsersResponseSchema.parse(json);
}

export async function fetchAdminUser(
	adminToken: string,
	userId: string,
	signal?: AbortSignal,
): Promise<AdminUser> {
	const res = await fetch(`/api/admin/users/${userId}`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUserSchema.parse(json);
}

export async function createAdminUser(
	adminToken: string,
	payload: AdminUserCreateRequest,
	signal?: AbortSignal,
): Promise<AdminUser> {
	const res = await fetch("/api/admin/users", {
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
	return AdminUserSchema.parse(json);
}

export async function patchAdminUser(
	adminToken: string,
	userId: string,
	payload: AdminUserPatchRequest,
	signal?: AbortSignal,
): Promise<AdminUser> {
	const res = await fetch(`/api/admin/users/${userId}`, {
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
	return AdminUserSchema.parse(json);
}

export async function deleteAdminUser(
	adminToken: string,
	userId: string,
	signal?: AbortSignal,
): Promise<void> {
	const res = await fetch(`/api/admin/users/${userId}`, {
		method: "DELETE",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);
}

export async function resetAdminUserToken(
	adminToken: string,
	userId: string,
	signal?: AbortSignal,
): Promise<AdminUserTokenResponse> {
	const res = await fetch(`/api/admin/users/${userId}/reset-token`, {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminUserTokenResponseSchema.parse(json);
}
