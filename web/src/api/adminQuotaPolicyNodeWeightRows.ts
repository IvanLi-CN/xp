import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminQuotaPolicyWeightRowSourceSchema = z.enum([
	"explicit",
	"implicit_zero",
]);

export type AdminQuotaPolicyWeightRowSource = z.infer<
	typeof AdminQuotaPolicyWeightRowSourceSchema
>;

export const AdminQuotaPolicyNodeWeightRowSchema = z.object({
	user_id: z.string(),
	display_name: z.string(),
	priority_tier: z.enum(["p1", "p2", "p3"]),
	endpoint_ids: z.array(z.string()),
	stored_weight: z.number().int().nonnegative().max(65535).optional(),
	editor_weight: z.number().int().nonnegative().max(65535),
	source: AdminQuotaPolicyWeightRowSourceSchema,
});

export type AdminQuotaPolicyNodeWeightRow = z.infer<
	typeof AdminQuotaPolicyNodeWeightRowSchema
>;

export const AdminQuotaPolicyNodeWeightRowsResponseSchema = z.object({
	items: z.array(AdminQuotaPolicyNodeWeightRowSchema),
});

export type AdminQuotaPolicyNodeWeightRowsResponse = z.infer<
	typeof AdminQuotaPolicyNodeWeightRowsResponseSchema
>;

export async function fetchAdminQuotaPolicyNodeWeightRows(
	adminToken: string,
	nodeId: string,
	signal?: AbortSignal,
): Promise<AdminQuotaPolicyNodeWeightRowsResponse> {
	const res = await fetch(
		`/api/admin/quota-policy/nodes/${nodeId}/weight-rows`,
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
	return AdminQuotaPolicyNodeWeightRowsResponseSchema.parse(json);
}
