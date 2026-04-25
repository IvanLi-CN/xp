import { z } from "zod";

import { throwIfNotOk } from "./backendError";
import { type NodeQuotaReset, NodeQuotaResetSchema } from "./quotaReset";

export const AdminNodeEgressProbeSchema = z.object({
	public_ipv4: z.string().nullable().optional(),
	public_ipv6: z.string().nullable().optional(),
	selected_public_ip: z.string().nullable().optional(),
	country_code: z.string().nullable().optional(),
	geo_region: z.string().nullable().optional(),
	geo_city: z.string().nullable().optional(),
	geo_operator: z.string().nullable().optional(),
	subscription_region: z.enum([
		"japan",
		"hong_kong",
		"taiwan",
		"korea",
		"singapore",
		"us",
		"other",
	]),
	checked_at: z.string(),
	last_success_at: z.string().nullable().optional(),
	stale: z.boolean(),
	error_summary: z.string().nullable().optional(),
});

export type AdminNodeEgressProbe = z.infer<typeof AdminNodeEgressProbeSchema>;

export const AdminNodeSchema = z.object({
	node_id: z.string(),
	node_name: z.string(),
	api_base_url: z.string(),
	access_host: z.string(),
	quota_limit_bytes: z.number().int().nonnegative(),
	quota_reset: NodeQuotaResetSchema,
	egress_probe: AdminNodeEgressProbeSchema.optional(),
});

export type AdminNode = z.infer<typeof AdminNodeSchema>;

export const AdminNodesResponseSchema = z.object({
	items: z.array(AdminNodeSchema),
});

export type AdminNodesResponse = z.infer<typeof AdminNodesResponseSchema>;

export type AdminNodePatchRequest = {
	quota_limit_bytes?: number;
	quota_reset?: NodeQuotaReset;
};

const AdminNodeEgressProbeRefreshResponseSchema = z.object({
	node_id: z.string(),
	accepted: z.boolean(),
	egress_probe: AdminNodeEgressProbeSchema.optional(),
});

export type AdminNodeEgressProbeRefreshResponse = z.infer<
	typeof AdminNodeEgressProbeRefreshResponseSchema
>;

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

export async function refreshAdminNodeEgressProbe(
	adminToken: string,
	nodeId: string,
	signal?: AbortSignal,
): Promise<AdminNodeEgressProbeRefreshResponse> {
	const res = await fetch(`/api/admin/nodes/${nodeId}/egress-probe/refresh`, {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminNodeEgressProbeRefreshResponseSchema.parse(json);
}

export async function deleteAdminNode(
	adminToken: string,
	nodeId: string,
	signal?: AbortSignal,
): Promise<void> {
	const res = await fetch(`/api/admin/nodes/${nodeId}`, {
		method: "DELETE",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);
}
