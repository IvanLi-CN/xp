import { z } from "zod";

import { fetchJsonWithTimeout } from "./request";

export const AdminNodeVersionItemSchema = z.object({
	node_id: z.string(),
	xp_version: z.string().nullable(),
	error: z.string().nullable(),
});

export const AdminNodeVersionsResponseSchema = z.object({
	items: z.array(AdminNodeVersionItemSchema),
});

export type AdminNodeVersionItem = z.infer<typeof AdminNodeVersionItemSchema>;
export type AdminNodeVersionsResponse = z.infer<
	typeof AdminNodeVersionsResponseSchema
>;

export async function fetchAdminNodeVersions(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminNodeVersionsResponse> {
	return fetchJsonWithTimeout(
		"/api/admin/nodes/versions",
		{
			method: "GET",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
		(value) => AdminNodeVersionsResponseSchema.parse(value),
	);
}
