import { z } from "zod";

import { throwIfNotOk } from "./backendError";
import { type NodeQuotaReset, NodeQuotaResetSchema } from "./quotaReset";

export const AdminNodeSchema = z.object({
	node_id: z.string(),
	node_name: z.string(),
	api_base_url: z.string(),
	access_host: z.string(),
	quota_reset: NodeQuotaResetSchema,
});

export type AdminNode = z.infer<typeof AdminNodeSchema>;

export const AdminNodesResponseSchema = z.object({
	items: z.array(AdminNodeSchema),
});

export type AdminNodesResponse = z.infer<typeof AdminNodesResponseSchema>;

export type AdminNodePatchRequest = {
	quota_reset?: NodeQuotaReset;
};

export async function fetchAdminNodes(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminNodesResponse> {
	const res = await fetch("/api/admin/nodes", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminNodesResponseSchema.parse(json);
}

export async function fetchAdminNode(
	adminToken: string,
	nodeId: string,
	signal?: AbortSignal,
): Promise<AdminNode> {
	const res = await fetch(`/api/admin/nodes/${nodeId}`, {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminNodeSchema.parse(json);
}

export async function patchAdminNode(
	adminToken: string,
	nodeId: string,
	payload: AdminNodePatchRequest,
	signal?: AbortSignal,
): Promise<AdminNode> {
	const res = await fetch(`/api/admin/nodes/${nodeId}`, {
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
	return AdminNodeSchema.parse(json);
}
