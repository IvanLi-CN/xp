import { z } from "zod";

import { throwIfNotOk } from "./backendError";
import { fetchJsonWithTimeout } from "./request";

export const HistoryRepositoryCapacitySchema = z.object({
	quota_bytes: z.number(),
	used_bytes: z.number(),
	filesystem_available_bytes: z.number(),
});

export const SummaryRepairDiagnosticSchema = z.object({
	pending_count: z.number(),
	summary_cursor: z.string().nullable().optional(),
	pending_next_cursor: z.string().nullable().optional(),
	last_attempt_at: z.number().nullable().optional(),
	last_progress_at: z.number().nullable().optional(),
	last_result: z.enum([
		"idle",
		"in_progress",
		"progress",
		"explicit_unavailable",
		"response_changed",
		"no_progress",
		"transport_error",
		"storage_error",
	]),
	last_error_code: z.string().nullable().optional(),
	retry_after: z.number().nullable().optional(),
	attempt_count: z.number(),
});

export const HistoryRepositoryMemberSchema = z.object({
	identity: z.object({
		node_id: z.string(),
		ed25519_public_key: z.string(),
		x25519_relay_public_key: z.string(),
	}),
	lifecycle: z.enum(["syncing", "ready", "retired"]),
	catch_up_completed_at: z.number().optional(),
	ready_at: z.number().optional(),
	replica_converged: z.boolean(),
	capacity: HistoryRepositoryCapacitySchema,
});

const DeliveryWindowSchema = z.object({
	started_at: z.number(),
	ended_at: z.number(),
	elapsed_seconds: z.number(),
	generated_segments: z.number(),
	generated_bytes: z.number(),
	acknowledged_segments: z.number(),
	acknowledged_bytes: z.number(),
	applied_segments: z.number(),
	applied_bytes: z.number(),
	generated_segments_per_second_milli: z.number(),
	generated_bytes_per_second_milli: z.number(),
	acknowledged_segments_per_second_milli: z.number(),
	acknowledged_bytes_per_second_milli: z.number(),
	applied_segments_per_second_milli: z.number(),
	applied_bytes_per_second_milli: z.number(),
	source_ack_meets_ten_x_segments: z.boolean(),
	source_ack_meets_ten_x_bytes: z.boolean(),
	source_ack_meets_ten_x: z.boolean(),
	repository_apply_meets_ten_x_segments: z.boolean(),
	repository_apply_meets_ten_x_bytes: z.boolean(),
	repository_apply_meets_ten_x: z.boolean(),
	meets_ten_x: z.boolean(),
});

const DeliveryScopeSchema = z.object({
	source_node_id: z.string(),
	stream: z.string(),
	target_repository_id: z.string(),
	generated_segments: z.number(),
	generated_bytes: z.number(),
	acknowledged_segments: z.number(),
	acknowledged_bytes: z.number(),
	applied_segments: z.number(),
	applied_bytes: z.number(),
	last_completed_window: DeliveryWindowSchema.nullable().optional(),
});

export const HistoryRepositoryRuntimeSchema = z.object({
	storage_mode: z.enum(["sqlite", "sqlite_degraded", "degraded_json"]),
	capacity: HistoryRepositoryCapacitySchema,
	record_count: z.number(),
	segment_count: z.number(),
	gap_count: z.number(),
	history_truncated: z.boolean(),
	last_verified_unix_seconds: z.number().nullable(),
	last_anti_entropy_unix_seconds: z.number().nullable(),
	last_deep_verification_unix_seconds: z.number().nullable(),
	last_dynamic_relay_attempt_unix_seconds: z.number().nullable(),
	delivery_metrics: z
		.object({
			generated_segments: z.number(),
			generated_bytes: z.number(),
			acknowledged_segments: z.number(),
			acknowledged_bytes: z.number(),
			applied_segments: z.number(),
			applied_bytes: z.number(),
			source_delivery_attempts: z.number(),
			source_delivery_successes: z.number(),
			source_transport_errors: z.number(),
			source_response_errors: z.number(),
			source_response_changed: z.number(),
			source_explicit_unavailable: z.number(),
			source_storage_errors: z.number(),
			last_attempt_at: z.number().nullable().optional(),
			last_progress_at: z.number().nullable().optional(),
			counter_epoch: z.number(),
			rate_window_seconds: z.number(),
			last_completed_window: DeliveryWindowSchema.nullable().optional(),
			delivery_scopes: z.array(DeliveryScopeSchema).optional(),
		})
		.optional(),
	source_delivery: z
		.object({
			state: z.enum([
				"idle",
				"backlogged",
				"journal_order_repairing",
				"source_storage_guard",
				"journal_capacity_guard",
				"journal_unavailable",
			]),
			pending_segments: z.number(),
			pending_bytes: z.number(),
			oldest_pending_cursor: z.string().nullable().optional(),
			oldest_pending_age_seconds: z.number().nullable().optional(),
			last_acknowledged_at: z.number().nullable().optional(),
			last_delivery_path: z.string().nullable().optional(),
		})
		.optional(),
	summary_repairs: z
		.record(z.string(), SummaryRepairDiagnosticSchema)
		.optional(),
});

export const AdminHistoryRepositoriesResponseSchema = z.object({
	configured: z.boolean(),
	partial: z.boolean(),
	unreachable_node_ids: z.array(z.string()),
	items: z.array(
		z.object({
			member: HistoryRepositoryMemberSchema,
			runtime: HistoryRepositoryRuntimeSchema.optional(),
		}),
	),
});

export type HistoryRepositoryMember = z.infer<
	typeof HistoryRepositoryMemberSchema
>;
export type HistoryRepositoryRuntime = z.infer<
	typeof HistoryRepositoryRuntimeSchema
>;
export type AdminHistoryRepositoriesResponse = z.infer<
	typeof AdminHistoryRepositoriesResponseSchema
>;

export async function fetchAdminHistoryRepositories(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminHistoryRepositoriesResponse> {
	return fetchJsonWithTimeout(
		"/api/admin/history-repositories",
		{
			method: "GET",
			headers: {
				Accept: "application/json",
				Authorization: `Bearer ${adminToken}`,
			},
			signal,
		},
		(value) => AdminHistoryRepositoriesResponseSchema.parse(value),
	);
}

export async function replaceAdminHistoryRepositories(
	adminToken: string,
	nodeIds: string[],
): Promise<HistoryRepositoryMember[]> {
	const response = await fetch("/api/admin/history-repositories", {
		method: "PUT",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ node_ids: nodeIds }),
	});
	await throwIfNotOk(response);
	const json: unknown = await response.json();
	return z
		.object({ members: z.array(HistoryRepositoryMemberSchema) })
		.parse(json).members;
}
