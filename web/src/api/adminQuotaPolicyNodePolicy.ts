import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminQuotaPolicyNodePolicySchema = z.object({
	node_id: z.string(),
	inherit_global: z.boolean(),
});

export type AdminQuotaPolicyNodePolicy = z.infer<
	typeof AdminQuotaPolicyNodePolicySchema
>;

export async function fetchAdminQuotaPolicyNodePolicy(
	adminToken: string,
	nodeId: string,
	signal?: AbortSignal,
): Promise<AdminQuotaPolicyNodePolicy> {
	const res = await fetch(`/api/admin/quota-policy/nodes/${nodeId}/policy`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);
	const json: unknown = await res.json();
	return AdminQuotaPolicyNodePolicySchema.parse(json);
}

export async function putAdminQuotaPolicyNodePolicy(
	adminToken: string,
	nodeId: string,
	inheritGlobal: boolean,
	signal?: AbortSignal,
): Promise<AdminQuotaPolicyNodePolicy> {
	const res = await fetch(`/api/admin/quota-policy/nodes/${nodeId}/policy`, {
		method: "PUT",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ inherit_global: inheritGlobal }),
		signal,
	});

	await throwIfNotOk(res);
	const json: unknown = await res.json();
	return AdminQuotaPolicyNodePolicySchema.parse(json);
}
