import { z } from "zod";

import { throwIfNotOk } from "./backendError";
import { GrantCredentialsSchema } from "./grantCredentials";

export const AdminGrantGroupSchema = z.object({
	group_name: z.string(),
});

export type AdminGrantGroup = z.infer<typeof AdminGrantGroupSchema>;

export const AdminGrantGroupSummarySchema = AdminGrantGroupSchema.extend({
	member_count: z.number().int().nonnegative(),
});

export type AdminGrantGroupSummary = z.infer<
	typeof AdminGrantGroupSummarySchema
>;

export const AdminGrantGroupMemberSchema = z.object({
	user_id: z.string(),
	endpoint_id: z.string(),
	enabled: z.boolean(),
	quota_limit_bytes: z.number().int().nonnegative(),
	note: z.string().nullable(),
	credentials: GrantCredentialsSchema,
});

export type AdminGrantGroupMember = z.infer<typeof AdminGrantGroupMemberSchema>;

export const AdminGrantGroupDetailSchema = z.object({
	group: AdminGrantGroupSchema,
	members: z.array(AdminGrantGroupMemberSchema),
});

export type AdminGrantGroupDetail = z.infer<typeof AdminGrantGroupDetailSchema>;

export const AdminGrantGroupsResponseSchema = z.object({
	items: z.array(AdminGrantGroupSummarySchema),
});

export type AdminGrantGroupsResponse = z.infer<
	typeof AdminGrantGroupsResponseSchema
>;

export type AdminGrantGroupCreateRequest = {
	group_name: string;
	members: Array<{
		user_id: string;
		endpoint_id: string;
		enabled: boolean;
		quota_limit_bytes: number;
		note?: string | null;
	}>;
};

export type AdminGrantGroupReplaceRequest = {
	rename_to?: string;
	members: Array<{
		user_id: string;
		endpoint_id: string;
		enabled: boolean;
		quota_limit_bytes: number;
		note?: string | null;
	}>;
};

export const AdminGrantGroupReplaceResponseSchema = z.object({
	group: AdminGrantGroupSchema,
	created: z.number().int().nonnegative(),
	updated: z.number().int().nonnegative(),
	deleted: z.number().int().nonnegative(),
});

export type AdminGrantGroupReplaceResponse = z.infer<
	typeof AdminGrantGroupReplaceResponseSchema
>;

export const AdminGrantGroupDeleteResponseSchema = z.object({
	deleted: z.number().int().nonnegative(),
});

export type AdminGrantGroupDeleteResponse = z.infer<
	typeof AdminGrantGroupDeleteResponseSchema
>;

export async function fetchAdminGrantGroups(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminGrantGroupsResponse> {
	const res = await fetch("/api/admin/grant-groups", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminGrantGroupsResponseSchema.parse(json);
}

export async function createAdminGrantGroup(
	adminToken: string,
	payload: AdminGrantGroupCreateRequest,
	signal?: AbortSignal,
): Promise<AdminGrantGroupDetail> {
	const res = await fetch("/api/admin/grant-groups", {
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
	return AdminGrantGroupDetailSchema.parse(json);
}

export async function fetchAdminGrantGroup(
	adminToken: string,
	groupName: string,
	signal?: AbortSignal,
): Promise<AdminGrantGroupDetail> {
	const res = await fetch(
		`/api/admin/grant-groups/${encodeURIComponent(groupName)}`,
		{
			method: "GET",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
	);

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminGrantGroupDetailSchema.parse(json);
}

export async function replaceAdminGrantGroup(
	adminToken: string,
	groupName: string,
	payload: AdminGrantGroupReplaceRequest,
	signal?: AbortSignal,
): Promise<AdminGrantGroupReplaceResponse> {
	const res = await fetch(
		`/api/admin/grant-groups/${encodeURIComponent(groupName)}`,
		{
			method: "PUT",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
				"Content-Type": "application/json",
			},
			body: JSON.stringify(payload),
			signal,
		},
	);

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminGrantGroupReplaceResponseSchema.parse(json);
}

export async function deleteAdminGrantGroup(
	adminToken: string,
	groupName: string,
	signal?: AbortSignal,
): Promise<AdminGrantGroupDeleteResponse> {
	const res = await fetch(
		`/api/admin/grant-groups/${encodeURIComponent(groupName)}`,
		{
			method: "DELETE",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
	);

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminGrantGroupDeleteResponseSchema.parse(json);
}
