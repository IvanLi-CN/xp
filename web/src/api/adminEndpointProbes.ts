import { z } from "zod";

import { EndpointProbeStatusSchema } from "./adminEndpoints";
import { throwIfNotOk } from "./backendError";

export const AdminEndpointProbeRunNodeSchema = z.object({
	node_id: z.string(),
	accepted: z.boolean(),
	already_running: z.boolean(),
	error: z.string().optional(),
});

export type AdminEndpointProbeRunNode = z.infer<
	typeof AdminEndpointProbeRunNodeSchema
>;

export const AdminEndpointProbeRunResponseSchema = z.object({
	run_id: z.string(),
	hour: z.string(),
	config_hash: z.string(),
	nodes: z.array(AdminEndpointProbeRunNodeSchema),
});

export type AdminEndpointProbeRunResponse = z.infer<
	typeof AdminEndpointProbeRunResponseSchema
>;

export const AdminEndpointProbeHistoryNodeSchema = z.object({
	node_id: z.string(),
	ok: z.boolean(),
	checked_at: z.string(),
	latency_ms: z.number().int().nonnegative().optional(),
	target_id: z.string().optional(),
	target_url: z.string().optional(),
	error: z.string().optional(),
	config_hash: z.string(),
});

export type AdminEndpointProbeHistoryNode = z.infer<
	typeof AdminEndpointProbeHistoryNodeSchema
>;

export const AdminEndpointProbeHistorySlotSchema = z.object({
	hour: z.string(),
	status: EndpointProbeStatusSchema,
	ok_count: z.number().int().nonnegative(),
	sample_count: z.number().int().nonnegative(),
	latency_ms_p50: z.number().int().nonnegative().optional(),
	latency_ms_p95: z.number().int().nonnegative().optional(),
	by_node: z.array(AdminEndpointProbeHistoryNodeSchema),
});

export type AdminEndpointProbeHistorySlot = z.infer<
	typeof AdminEndpointProbeHistorySlotSchema
>;

export const AdminEndpointProbeHistoryResponseSchema = z.object({
	endpoint_id: z.string(),
	expected_nodes: z.number().int().nonnegative(),
	slots: z.array(AdminEndpointProbeHistorySlotSchema),
});

export type AdminEndpointProbeHistoryResponse = z.infer<
	typeof AdminEndpointProbeHistoryResponseSchema
>;

export async function runAdminEndpointProbeRun(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminEndpointProbeRunResponse> {
	const res = await fetch("/api/admin/endpoints/probe/run", {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminEndpointProbeRunResponseSchema.parse(json);
}

export async function fetchAdminEndpointProbeHistory(
	adminToken: string,
	endpointId: string,
	hours = 24,
	signal?: AbortSignal,
): Promise<AdminEndpointProbeHistoryResponse> {
	const query = new URLSearchParams({ hours: String(hours) });
	const res = await fetch(
		`/api/admin/endpoints/${encodeURIComponent(endpointId)}/probe-history?${query}`,
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
	return AdminEndpointProbeHistoryResponseSchema.parse(json);
}
