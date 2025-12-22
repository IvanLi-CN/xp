import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const CyclePolicySchema = z.enum(["inherit_user", "by_user", "by_node"]);

export type CyclePolicy = z.infer<typeof CyclePolicySchema>;

export const VlessCredentialsSchema = z.object({
	uuid: z.string(),
	email: z.string(),
});

export type VlessCredentials = z.infer<typeof VlessCredentialsSchema>;

export const Ss2022CredentialsSchema = z.object({
	method: z.string(),
	password: z.string(),
});

export type Ss2022Credentials = z.infer<typeof Ss2022CredentialsSchema>;

export const GrantCredentialsSchema = z.object({
	vless: VlessCredentialsSchema.optional(),
	ss2022: Ss2022CredentialsSchema.optional(),
});

export type GrantCredentials = z.infer<typeof GrantCredentialsSchema>;

export const AdminGrantSchema = z.object({
	grant_id: z.string(),
	user_id: z.string(),
	endpoint_id: z.string(),
	enabled: z.boolean(),
	quota_limit_bytes: z.number().int().nonnegative(),
	cycle_policy: CyclePolicySchema,
	cycle_day_of_month: z.number().int().min(1).max(31).nullable(),
	note: z.string().nullable(),
	credentials: GrantCredentialsSchema,
});

export type AdminGrant = z.infer<typeof AdminGrantSchema>;

export const AdminGrantsResponseSchema = z.object({
	items: z.array(AdminGrantSchema),
});

export type AdminGrantsResponse = z.infer<typeof AdminGrantsResponseSchema>;

export const AdminGrantUsageResponseSchema = z.object({
	grant_id: z.string(),
	cycle_start_at: z.string(),
	cycle_end_at: z.string(),
	used_bytes: z.number().int().nonnegative(),
	owner_node_id: z.string(),
	desired_enabled: z.boolean(),
	quota_banned: z.boolean(),
	quota_banned_at: z.string().nullable(),
	effective_enabled: z.boolean(),
	warning: z.string().nullable(),
});

export type AdminGrantUsageResponse = z.infer<
	typeof AdminGrantUsageResponseSchema
>;

export type AdminGrantCreateRequest = {
	user_id: string;
	endpoint_id: string;
	quota_limit_bytes: number;
	cycle_policy: CyclePolicy;
	cycle_day_of_month: number | null;
	note?: string | null;
};

export type AdminGrantPatchRequest = {
	enabled: boolean;
	quota_limit_bytes: number;
	cycle_policy: CyclePolicy;
	cycle_day_of_month: number | null;
	note?: string | null;
};

export async function fetchAdminGrants(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminGrantsResponse> {
	const res = await fetch("/api/admin/grants", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminGrantsResponseSchema.parse(json);
}

export async function fetchAdminGrant(
	adminToken: string,
	grantId: string,
	signal?: AbortSignal,
): Promise<AdminGrant> {
	const res = await fetch(`/api/admin/grants/${grantId}`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminGrantSchema.parse(json);
}

export async function createAdminGrant(
	adminToken: string,
	payload: AdminGrantCreateRequest,
	signal?: AbortSignal,
): Promise<AdminGrant> {
	const res = await fetch("/api/admin/grants", {
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
	return AdminGrantSchema.parse(json);
}

export async function patchAdminGrant(
	adminToken: string,
	grantId: string,
	payload: AdminGrantPatchRequest,
	signal?: AbortSignal,
): Promise<AdminGrant> {
	const res = await fetch(`/api/admin/grants/${grantId}`, {
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
	return AdminGrantSchema.parse(json);
}

export async function deleteAdminGrant(
	adminToken: string,
	grantId: string,
	signal?: AbortSignal,
): Promise<void> {
	const res = await fetch(`/api/admin/grants/${grantId}`, {
		method: "DELETE",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);
}

export async function fetchAdminGrantUsage(
	adminToken: string,
	grantId: string,
	signal?: AbortSignal,
): Promise<AdminGrantUsageResponse> {
	const res = await fetch(`/api/admin/grants/${grantId}/usage`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminGrantUsageResponseSchema.parse(json);
}
