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

export const AdminEndpointProbeRunProgressStatusSchema = z.enum([
	"running",
	"finished",
	"failed",
]);

export type AdminEndpointProbeRunProgressStatus = z.infer<
	typeof AdminEndpointProbeRunProgressStatusSchema
>;

export const AdminEndpointProbeRunProgressSchema = z.object({
	run_id: z.string(),
	hour: z.string(),
	config_hash: z.string(),
	status: AdminEndpointProbeRunProgressStatusSchema,
	endpoints_total: z.number().int().nonnegative(),
	endpoints_done: z.number().int().nonnegative(),
	started_at: z.string(),
	updated_at: z.string(),
	finished_at: z.string().optional(),
	error: z.string().optional(),
});

export type AdminEndpointProbeRunProgress = z.infer<
	typeof AdminEndpointProbeRunProgressSchema
>;

export const AdminEndpointProbeRunNodeStatusSchema = z.enum([
	"running",
	"finished",
	"failed",
	"busy",
	"not_found",
	"unknown",
]);

export type AdminEndpointProbeRunNodeStatus = z.infer<
	typeof AdminEndpointProbeRunNodeStatusSchema
>;

export const AdminEndpointProbeRunOverallStatusSchema = z.enum([
	"running",
	"finished",
	"failed",
	"not_found",
	"unknown",
]);

export type AdminEndpointProbeRunOverallStatus = z.infer<
	typeof AdminEndpointProbeRunOverallStatusSchema
>;

export const AdminEndpointProbeRunStatusNodeSchema = z.object({
	node_id: z.string(),
	status: AdminEndpointProbeRunNodeStatusSchema,
	progress: AdminEndpointProbeRunProgressSchema.optional(),
	current: AdminEndpointProbeRunProgressSchema.optional(),
	error: z.string().optional(),
});

export type AdminEndpointProbeRunStatusNode = z.infer<
	typeof AdminEndpointProbeRunStatusNodeSchema
>;

export const AdminEndpointProbeRunStatusResponseSchema = z.object({
	run_id: z.string(),
	status: AdminEndpointProbeRunOverallStatusSchema,
	hour: z.string().optional(),
	config_hash: z.string().optional(),
	nodes: z.array(AdminEndpointProbeRunStatusNodeSchema),
});

export type AdminEndpointProbeRunStatusResponse = z.infer<
	typeof AdminEndpointProbeRunStatusResponseSchema
>;

export async function fetchAdminEndpointProbeRunStatus(
	adminToken: string,
	runId: string,
	signal?: AbortSignal,
): Promise<AdminEndpointProbeRunStatusResponse> {
	const res = await fetch(
		`/api/admin/endpoints/probe/runs/${encodeURIComponent(runId)}`,
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
	return AdminEndpointProbeRunStatusResponseSchema.parse(json);
}

export const EndpointProbeAppendSampleSchema = z.object({
	endpoint_id: z.string(),
	ok: z.boolean(),
	checked_at: z.string(),
	latency_ms: z.number().int().nonnegative().nullable().optional(),
	target_id: z.string().nullable().optional(),
	target_url: z.string().nullable().optional(),
	error: z.string().nullable().optional(),
	config_hash: z.string(),
});

export type EndpointProbeAppendSample = z.infer<
	typeof EndpointProbeAppendSampleSchema
>;

export const AdminEndpointProbeRunSseHelloSchema = z.object({
	run_id: z.string(),
	connected_at: z.string(),
	nodes: z.array(z.string()),
});

export type AdminEndpointProbeRunSseHello = z.infer<
	typeof AdminEndpointProbeRunSseHelloSchema
>;

export const AdminEndpointProbeRunSseNodeProgressSchema = z.object({
	node_id: z.string(),
	progress: AdminEndpointProbeRunProgressSchema,
});

export type AdminEndpointProbeRunSseNodeProgress = z.infer<
	typeof AdminEndpointProbeRunSseNodeProgressSchema
>;

export const AdminEndpointProbeRunSseEndpointSampleSchema = z.object({
	node_id: z.string(),
	run_id: z.string(),
	hour: z.string(),
	sample: EndpointProbeAppendSampleSchema,
});

export type AdminEndpointProbeRunSseEndpointSample = z.infer<
	typeof AdminEndpointProbeRunSseEndpointSampleSchema
>;

export const AdminEndpointProbeRunSseNodeErrorSchema = z.object({
	node_id: z.string(),
	run_id: z.string(),
	error: z.string(),
});

export type AdminEndpointProbeRunSseNodeError = z.infer<
	typeof AdminEndpointProbeRunSseNodeErrorSchema
>;

export const AdminEndpointProbeRunSseLaggedSchema = z.object({
	node_id: z.string(),
	run_id: z.string(),
	missed: z.number().int().nonnegative(),
});

export type AdminEndpointProbeRunSseLagged = z.infer<
	typeof AdminEndpointProbeRunSseLaggedSchema
>;

export const AdminEndpointProbeRunSseNotFoundSchema = z.object({
	node_id: z.string(),
	run_id: z.string(),
});

export type AdminEndpointProbeRunSseNotFound = z.infer<
	typeof AdminEndpointProbeRunSseNotFoundSchema
>;
