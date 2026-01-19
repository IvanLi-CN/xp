import { z } from "zod";

import { GrantCredentialsSchema } from "./adminGrants";
import { throwIfNotOk } from "./backendError";

export const AdminGrantGroupSchema = z.object({
	group_name: z.string(),
});

export type AdminGrantGroup = z.infer<typeof AdminGrantGroupSchema>;

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
