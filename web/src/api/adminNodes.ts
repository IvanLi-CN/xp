import { z } from "zod";

import { throwIfNotOk } from "./backendError";
import { type NodeQuotaReset, NodeQuotaResetSchema } from "./quotaReset";

export const AdminNodeSchema = z.object({
	node_id: z.string(),
	node_name: z.string(),
	api_base_url: z.string(),
	access_host: z.string(),
	quota_reset: NodeQuotaResetSchema,
	quota_limit_bytes: z.number().int().nonnegative(),
});

export type AdminNode = z.infer<typeof AdminNodeSchema>;

export const AdminNodesResponseSchema = z.object({
	items: z.array(AdminNodeSchema),
});

export type AdminNodesResponse = z.infer<typeof AdminNodesResponseSchema>;

export type AdminNodePatchRequest = {
	quota_reset?: NodeQuotaReset;
	quota_limit_bytes?: number;
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

export const AdminNodeQuotaStatusItemSchema = z.object({
	node_id: z.string(),
	quota_limit_bytes: z.number().int().nonnegative(),
	used_bytes: z.number().int().nonnegative(),
	remaining_bytes: z.number().int().nonnegative(),
	cycle_end_at: z.string().nullable(),
	exhausted: z.boolean(),
	exhausted_at: z.string().nullable(),
	warning: z.string().nullable(),
});

export type AdminNodeQuotaStatusItem = z.infer<
	typeof AdminNodeQuotaStatusItemSchema
>;

export const AdminNodeQuotaStatusResponseSchema = z.object({
	partial: z.boolean(),
	unreachable_nodes: z.array(z.string()),
	items: z.array(AdminNodeQuotaStatusItemSchema),
});

export type AdminNodeQuotaStatusResponse = z.infer<
	typeof AdminNodeQuotaStatusResponseSchema
>;

export async function fetchAdminNodeQuotaStatus(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminNodeQuotaStatusResponse> {
	const res = await fetch("/api/admin/nodes/quota-status", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminNodeQuotaStatusResponseSchema.parse(json);
}

export type AdminNodeQuotaUsageOverrideRequest = {
	used_bytes: number;
	sync_baseline?: boolean;
};

export const AdminNodeQuotaUsageOverrideResponseSchema = z.object({
	status: AdminNodeQuotaStatusItemSchema,
	synced_baseline: z.boolean(),
	warning: z.string().nullable(),
});

export type AdminNodeQuotaUsageOverrideResponse = z.infer<
	typeof AdminNodeQuotaUsageOverrideResponseSchema
>;

export async function patchAdminNodeQuotaUsage(
	adminToken: string,
	nodeId: string,
	payload: AdminNodeQuotaUsageOverrideRequest,
	signal?: AbortSignal,
): Promise<AdminNodeQuotaUsageOverrideResponse> {
	const res = await fetch(`/api/admin/nodes/${nodeId}/quota-usage`, {
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
	return AdminNodeQuotaUsageOverrideResponseSchema.parse(json);
}
