import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const HistoryRepositoryCapacitySchema = z.object({
	quota_bytes: z.number(),
	used_bytes: z.number(),
	filesystem_available_bytes: z.number(),
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

export const HistoryRepositoryRuntimeSchema = z.object({
	storage_mode: z.enum(["sqlite", "degraded_json"]),
	capacity: HistoryRepositoryCapacitySchema,
	record_count: z.number(),
	segment_count: z.number(),
	gap_count: z.number(),
	history_truncated: z.boolean(),
	last_verified_unix_seconds: z.number().nullable(),
	last_anti_entropy_unix_seconds: z.number().nullable(),
	last_deep_verification_unix_seconds: z.number().nullable(),
	last_dynamic_relay_attempt_unix_seconds: z.number().nullable(),
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
	const response = await fetch("/api/admin/history-repositories", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});
	await throwIfNotOk(response);
	return AdminHistoryRepositoriesResponseSchema.parse(await response.json());
}

export async function replaceAdminHistoryRepositories(
	adminToken: string,
	members: HistoryRepositoryMember[],
): Promise<HistoryRepositoryMember[]> {
	const response = await fetch("/api/admin/history-repositories", {
		method: "PUT",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ members }),
	});
	await throwIfNotOk(response);
	const json: unknown = await response.json();
	return z
		.object({ members: z.array(HistoryRepositoryMemberSchema) })
		.parse(json).members;
}
