import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminQuotaPolicyGlobalWeightRowSourceSchema = z.enum([
	"explicit",
	"implicit_default",
]);

export type AdminQuotaPolicyGlobalWeightRowSource = z.infer<
	typeof AdminQuotaPolicyGlobalWeightRowSourceSchema
>;

export const AdminQuotaPolicyGlobalWeightRowSchema = z.object({
	user_id: z.string(),
	display_name: z.string(),
	priority_tier: z.enum(["p1", "p2", "p3"]),
	stored_weight: z.number().int().nonnegative().max(65535).optional(),
	editor_weight: z.number().int().nonnegative().max(65535),
	source: AdminQuotaPolicyGlobalWeightRowSourceSchema,
});

export type AdminQuotaPolicyGlobalWeightRow = z.infer<
	typeof AdminQuotaPolicyGlobalWeightRowSchema
>;

export const AdminQuotaPolicyGlobalWeightRowsResponseSchema = z.object({
	items: z.array(AdminQuotaPolicyGlobalWeightRowSchema),
});

export type AdminQuotaPolicyGlobalWeightRowsResponse = z.infer<
	typeof AdminQuotaPolicyGlobalWeightRowsResponseSchema
>;

export async function fetchAdminQuotaPolicyGlobalWeightRows(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminQuotaPolicyGlobalWeightRowsResponse> {
	const res = await fetch("/api/admin/quota-policy/global-weight-rows", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminQuotaPolicyGlobalWeightRowsResponseSchema.parse(json);
}

export const PutAdminQuotaPolicyGlobalWeightRowResponseSchema = z.object({
	user_id: z.string(),
	weight: z.number().int().nonnegative().max(65535),
});

export type PutAdminQuotaPolicyGlobalWeightRowResponse = z.infer<
	typeof PutAdminQuotaPolicyGlobalWeightRowResponseSchema
>;

export async function putAdminQuotaPolicyGlobalWeightRow(
	adminToken: string,
	userId: string,
	weight: number,
	signal?: AbortSignal,
): Promise<PutAdminQuotaPolicyGlobalWeightRowResponse> {
	const res = await fetch(
		`/api/admin/quota-policy/global-weight-rows/${userId}`,
		{
			method: "PUT",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
				"Content-Type": "application/json",
			},
			body: JSON.stringify({ weight }),
			signal,
		},
	);

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return PutAdminQuotaPolicyGlobalWeightRowResponseSchema.parse(json);
}
