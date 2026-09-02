import { z } from "zod";

import { throwIfNotOk } from "./backendError";

const StatusRangeSchema = z.object({
	start: z.number().int(),
	end: z.number().int(),
});

export const ServiceMonitorTargetSchema = z.discriminatedUnion("kind", [
	z.object({
		kind: z.literal("http"),
		url: z.string().url(),
		method: z.enum(["get", "head"]),
		accepted_statuses: z.array(StatusRangeSchema),
		body_contains: z.string().nullable().optional(),
	}),
	z.object({
		kind: z.literal("https"),
		url: z.string().url(),
		method: z.enum(["get", "head"]),
		accepted_statuses: z.array(StatusRangeSchema),
		body_contains: z.string().nullable().optional(),
	}),
	z.object({ kind: z.literal("ping"), host: z.string().min(1) }),
	z.object({
		kind: z.literal("tcping"),
		host: z.string().min(1),
		port: z.number().int(),
	}),
]);

export type ServiceMonitorTarget = z.infer<typeof ServiceMonitorTargetSchema>;
export type ServiceMonitorKind = "http" | "https" | "ping" | "tcping";
export type ServiceMonitorLifecycle = "active" | "paused" | "deleted";
export type ServiceMonitorStatus =
	| "up"
	| "degraded"
	| "down"
	| "unknown"
	| "capture_suspended";
export type HistoryQuality = "complete" | "partial" | "local_only";

export const ObserverPolicySchema = z.object({
	mode: z.enum(["exclude", "include"]),
	node_ids: z.array(z.string()),
});

export type ObserverPolicy = z.infer<typeof ObserverPolicySchema>;

const AdminServiceMonitorWireSchema = z.object({
	monitor_id: z.string(),
	name: z.string(),
	target: ServiceMonitorTargetSchema,
	interval_seconds: z.number().int(),
	observer_policy: ObserverPolicySchema.optional(),
	observer_node_ids: z.array(z.string()).nullable().optional(),
	lifecycle: z.enum(["active", "paused", "deleted"]),
	revision: z.number().int(),
	revision_effective_at_unix_seconds: z.number().int(),
});

export const AdminServiceMonitorSchema =
	AdminServiceMonitorWireSchema.transform(
		({ observer_policy, observer_node_ids, ...monitor }) => ({
			...monitor,
			observer_policy:
				observer_policy ??
				(observer_node_ids && observer_node_ids.length > 0
					? { mode: "include" as const, node_ids: observer_node_ids }
					: { mode: "exclude" as const, node_ids: [] }),
		}),
	);

export type AdminServiceMonitor = z.infer<typeof AdminServiceMonitorSchema>;

export const ServiceMonitorSummarySchema = AdminServiceMonitorWireSchema.extend(
	{
		status: z.enum(["up", "degraded", "down", "unknown", "capture_suspended"]),
		stale: z.boolean(),
		quality: z.enum(["complete", "partial", "local_only"]),
		recent_6h: z.object({
			availability_percent: z.number().nullable(),
			coverage_percent: z.number().nullable(),
			expected: z.number().int(),
			executed: z.number().int(),
			latest_latency_ms: z.number().int().nullable(),
			latest_observed_at_unix_seconds: z.number().int().nullable(),
			slots: z.array(
				z.enum(["up", "degraded", "down", "unknown", "capture_suspended"]),
			),
		}),
	},
).transform(({ observer_policy, observer_node_ids, ...monitor }) => ({
	...monitor,
	observer_policy:
		observer_policy ??
		(observer_node_ids && observer_node_ids.length > 0
			? { mode: "include" as const, node_ids: observer_node_ids }
			: { mode: "exclude" as const, node_ids: [] }),
}));

export type ServiceMonitorSummary = z.infer<typeof ServiceMonitorSummarySchema>;

const ObservationSchema = z.object({
	monitor_id: z.string(),
	revision: z.number().int(),
	observer_node_id: z.string(),
	slot_unix_seconds: z.number().int(),
	observed_at_unix_seconds: z.number().int(),
	outcome: z.enum(["success", "failure", "unsupported", "suspended"]),
	error: z.string().nullable().optional(),
	latency_ms: z.number().int().nullable().optional(),
	status_code: z.number().int().nullable().optional(),
	packet_loss_percent: z.number().int(),
	ad_hoc: z.boolean(),
});

export type ServiceMonitorObservation = z.infer<typeof ObservationSchema>;

const CaptureStateSchema = z.object({
	suspended: z.boolean(),
	pending_observations: z.number().int(),
	pending_bytes: z.number().int(),
});

export const ServiceMonitorStatusResponseSchema = z.object({
	monitor_id: z.string(),
	status: z.enum(["up", "degraded", "down", "unknown", "capture_suspended"]),
	stale: z.boolean(),
	freshness_seconds: z.number().int().nullable().optional(),
	capture: CaptureStateSchema,
	quality: z.enum(["complete", "partial", "local_only"]),
	observers: z.array(
		z.object({
			node_id: z.string(),
			state: z.enum(["up", "degraded", "down", "unknown", "capture_suspended"]),
			latest: ObservationSchema.nullable().optional(),
			icmp_supported: z.boolean().nullable().optional(),
		}),
	),
});

export type ServiceMonitorStatusResponse = z.infer<
	typeof ServiceMonitorStatusResponseSchema
>;

const LatencyHistogramSchema = z.object({
	underflow: z.number().int(),
	buckets: z.array(z.number().int()),
	overflow: z.number().int(),
});

const ObservationRollupSchema = z.object({
	expected: z.number().int(),
	executed: z.number().int(),
	successes: z.number().int(),
	failures: z.number().int(),
	unsupported: z.number().int(),
	suspended: z.number().int(),
	latency_count: z.number().int(),
	latency_sum_ms: z.number().int(),
	latency_min_ms: z.number().int().nullable().optional(),
	latency_max_ms: z.number().int().nullable().optional(),
	latency_histogram: LatencyHistogramSchema,
	errors: z.record(z.string(), z.number().int()),
});

export const ServiceMonitorHistoryResponseSchema = z.object({
	monitor_id: z.string(),
	resolution: z.string(),
	points: z.array(
		z.object({
			start_unix_seconds: z.number().int(),
			end_unix_seconds: z.number().int(),
			rollup: ObservationRollupSchema,
			availability_percent: z.number().nullable(),
			coverage_percent: z.number().nullable(),
		}),
	),
	truncated: z.boolean(),
	quality: z.enum(["complete", "partial", "local_only"]),
	coverage_percent: z.number().nullable(),
	watermark_unix_seconds: z.number().int().nullable().optional(),
	gaps: z.array(
		z.object({
			start_unix_seconds: z.number().int(),
			end_unix_seconds: z.number().int(),
			expected: z.number().int(),
			executed: z.number().int(),
		}),
	),
	skew_seconds: z.number().int(),
	freshness_seconds: z.number().int().nullable().optional(),
});

export type ServiceMonitorHistoryResponse = z.infer<
	typeof ServiceMonitorHistoryResponseSchema
>;

export const AdHocRunSchema = z.object({
	run_id: z.string(),
	monitor_id: z.string(),
	state: z.enum(["queued", "running", "succeeded", "failed", "rejected"]),
	created_at_unix_seconds: z.number().int(),
	completed_at_unix_seconds: z.number().int().nullable().optional(),
	observation: ObservationSchema.nullable().optional(),
	reason: z.string().nullable().optional(),
});

export type ServiceMonitorCreateRequest = {
	name: string;
	target: ServiceMonitorTarget;
	interval_seconds: number;
	observer_policy?: ObserverPolicy;
	observer_node_ids?: string[] | null;
};

export type ServiceMonitorPatchRequest =
	Partial<ServiceMonitorCreateRequest> & {
		expected_revision: number;
		lifecycle?: Exclude<ServiceMonitorLifecycle, "deleted">;
	};

export const ServiceMonitorTestResponseSchema = z.object({
	target: ServiceMonitorTargetSchema,
	observations: z.array(ObservationSchema),
});

export type ServiceMonitorTestResponse = z.infer<
	typeof ServiceMonitorTestResponseSchema
>;

const DraftClusterTestStateSchema = z.enum([
	"queued",
	"running",
	"succeeded",
	"failed",
	"unsupported",
	"interrupted",
]);

export const DraftClusterTestSchema = z.object({
	run_id: z.string(),
	target: ServiceMonitorTargetSchema,
	observer_policy: ObserverPolicySchema,
	observer_node_ids: z.array(z.string()),
	coordinator_node_id: z.string(),
	state: DraftClusterTestStateSchema,
	created_at_unix_seconds: z.number().int(),
	expires_at_unix_seconds: z.number().int(),
	observers: z.array(
		z.object({
			node_id: z.string(),
			state: DraftClusterTestStateSchema,
			latency_ms: z.number().int().nullable().optional(),
			status_code: z.number().int().nullable().optional(),
			error: z.string().nullable().optional(),
			started_at_unix_seconds: z.number().int().nullable().optional(),
			completed_at_unix_seconds: z.number().int().nullable().optional(),
		}),
	),
});

export type DraftClusterTest = z.infer<typeof DraftClusterTestSchema>;

function authHeaders(adminToken: string, json = false): HeadersInit {
	return {
		Accept: "application/json",
		Authorization: `Bearer ${adminToken}`,
		...(json ? { "Content-Type": "application/json" } : {}),
	};
}

async function fetchJson<T>(
	url: string,
	adminToken: string,
	parse: (value: unknown) => T,
	init?: RequestInit,
): Promise<T> {
	const res = await fetch(url, {
		...init,
		headers: authHeaders(adminToken, init?.body !== undefined),
	});
	await throwIfNotOk(res);
	return parse((await res.json()) as unknown);
}

export function monitorKind(target: ServiceMonitorTarget): ServiceMonitorKind {
	return target.kind;
}

export function monitorTargetLabel(target: ServiceMonitorTarget): string {
	if (target.kind === "http" || target.kind === "https") return target.url;
	if (target.kind === "ping") return target.host;
	return `${target.host}:${target.port}`;
}

export async function fetchAdminServiceMonitors(
	adminToken: string,
	signal?: AbortSignal,
): Promise<{ items: ServiceMonitorSummary[] }> {
	return fetchJson(
		"/api/admin/monitors",
		adminToken,
		(value) =>
			z.object({ items: z.array(ServiceMonitorSummarySchema) }).parse(value),
		{ signal },
	);
}

export async function fetchAdminServiceMonitor(
	adminToken: string,
	monitorId: string,
	signal?: AbortSignal,
): Promise<AdminServiceMonitor> {
	return fetchJson(
		`/api/admin/monitors/${monitorId}`,
		adminToken,
		(value) => AdminServiceMonitorSchema.parse(value),
		{ signal },
	);
}

export async function createAdminServiceMonitor(
	adminToken: string,
	payload: ServiceMonitorCreateRequest,
): Promise<AdminServiceMonitor> {
	return fetchJson(
		"/api/admin/monitors",
		adminToken,
		(value) => AdminServiceMonitorSchema.parse(value),
		{ method: "POST", body: JSON.stringify(payload) },
	);
}

export async function testAdminServiceMonitorTarget(
	adminToken: string,
	target: ServiceMonitorTarget,
): Promise<ServiceMonitorTestResponse> {
	return fetchJson(
		"/api/admin/monitors/test",
		adminToken,
		(value) => ServiceMonitorTestResponseSchema.parse(value),
		{ method: "POST", body: JSON.stringify({ target }) },
	);
}

export async function createAdminMonitorDraftTest(
	adminToken: string,
	target: ServiceMonitorTarget,
	observerPolicy: ObserverPolicy,
): Promise<DraftClusterTest> {
	return fetchJson(
		"/api/admin/monitor-draft-tests",
		adminToken,
		(value) => DraftClusterTestSchema.parse(value),
		{
			method: "POST",
			body: JSON.stringify({ target, observer_policy: observerPolicy }),
		},
	);
}

export async function fetchAdminMonitorDraftTest(
	adminToken: string,
	runId: string,
	signal?: AbortSignal,
): Promise<DraftClusterTest> {
	return fetchJson(
		`/api/admin/monitor-draft-tests/${encodeURIComponent(runId)}`,
		adminToken,
		(value) => DraftClusterTestSchema.parse(value),
		{ signal },
	);
}

export async function patchAdminServiceMonitor(
	adminToken: string,
	monitorId: string,
	payload: ServiceMonitorPatchRequest,
): Promise<AdminServiceMonitor> {
	return fetchJson(
		`/api/admin/monitors/${monitorId}`,
		adminToken,
		(value) => AdminServiceMonitorSchema.parse(value),
		{ method: "PATCH", body: JSON.stringify(payload) },
	);
}

export async function deleteAdminServiceMonitor(
	adminToken: string,
	monitorId: string,
	expectedRevision: number,
): Promise<void> {
	const res = await fetch(
		`/api/admin/monitors/${monitorId}?expected_revision=${expectedRevision}`,
		{ method: "DELETE", headers: authHeaders(adminToken) },
	);
	await throwIfNotOk(res);
}

export async function runAdminServiceMonitor(
	adminToken: string,
	monitorId: string,
): Promise<{ run_id: string; state: "queued" | "running" }> {
	return fetchJson(
		`/api/admin/monitors/${monitorId}/run`,
		adminToken,
		(value) =>
			z
				.object({ run_id: z.string(), state: z.enum(["queued", "running"]) })
				.parse(value),
		{ method: "POST" },
	);
}

export async function fetchAdminServiceMonitorStatus(
	adminToken: string,
	monitorId: string,
	signal?: AbortSignal,
): Promise<ServiceMonitorStatusResponse> {
	return fetchJson(
		`/api/admin/monitors/${monitorId}/status`,
		adminToken,
		(value) => ServiceMonitorStatusResponseSchema.parse(value),
		{ signal },
	);
}

export async function fetchAdminServiceMonitorHistory(
	adminToken: string,
	monitorId: string,
	params: {
		from?: number;
		to?: number;
		resolution?: string;
		observerId?: string;
		limit?: number;
	},
	signal?: AbortSignal,
): Promise<ServiceMonitorHistoryResponse> {
	const search = new URLSearchParams();
	if (params.from !== undefined) search.set("from", String(params.from));
	if (params.to !== undefined) search.set("to", String(params.to));
	if (params.resolution) search.set("resolution", params.resolution);
	if (params.observerId) search.set("observer_id", params.observerId);
	if (params.limit !== undefined) search.set("limit", String(params.limit));
	return fetchJson(
		`/api/admin/monitors/${monitorId}/history?${search}`,
		adminToken,
		(value) => ServiceMonitorHistoryResponseSchema.parse(value),
		{ signal },
	);
}
