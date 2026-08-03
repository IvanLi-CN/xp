import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const MeshTelemetryPathSchema = z.enum(["mesh", "public"]);
export const MeshQualitySchema = z.enum([
	"good",
	"slow",
	"unstable",
	"down",
	"unknown",
]);
export const MeshBreakerStateSchema = z.enum([
	"closed",
	"open",
	"half_open",
	"disabled",
]);

export const AdminMeshBucketSchema = z.object({
	minute: z.string(),
	mesh_success: z.number(),
	mesh_failure: z.number(),
	public_success: z.number(),
	public_failure: z.number(),
	fallback_success: z.number(),
	end_to_end_success: z.number(),
	end_to_end_failure: z.number(),
	latency_samples_ms: z.array(z.number()),
});

export const AdminMeshPeerSchema = z.object({
	node_id: z.string(),
	node_name: z.string(),
	api_base_url: z.string(),
	mesh_url: z.string().nullable(),
	current_path: MeshTelemetryPathSchema.nullable(),
	quality: MeshQualitySchema,
	stale: z.boolean(),
	breaker: MeshBreakerStateSchema,
	last_sample_at: z.string().nullable(),
	last_transition_at: z.string().nullable(),
	availability_1h: z.number().nullable(),
	availability_24h: z.number().nullable(),
	mesh_availability_24h: z.number().nullable(),
	latency_p50_ms: z.number().nullable(),
	latency_p95_ms: z.number().nullable(),
	buckets: z.array(AdminMeshBucketSchema),
});

export const AdminMeshStatusSchema = z.object({
	generated_at: z.string(),
	revision: z.number(),
	local: z.object({
		node_id: z.string(),
		node_name: z.string(),
		cluster_id: z.string(),
		role: z.enum(["leader", "follower"]),
		leader_api_base_url: z.string(),
		term: z.number(),
		mesh_proxy_status: z.string(),
		mesh_proxy_reason: z.string().nullable(),
		canary: z.object({
			enabled: z.boolean(),
			bind: z.string().nullable().optional(),
			acme_directory_url: z.string().nullable().optional(),
			cert_not_after: z.string().nullable().optional(),
			last_renewed_at: z.string().nullable().optional(),
			last_error: z.string().nullable().optional(),
		}),
	}),
	peers: z.array(AdminMeshPeerSchema),
	events: z.array(
		z.object({
			at: z.string(),
			peer_id: z.string(),
			kind: z.string(),
			message: z.string(),
		}),
	),
});

export type AdminMeshStatus = z.infer<typeof AdminMeshStatusSchema>;
export type AdminMeshPeer = z.infer<typeof AdminMeshPeerSchema>;
export type AdminMeshBucket = z.infer<typeof AdminMeshBucketSchema>;

export async function fetchAdminMeshStatus(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminMeshStatus> {
	const response = await fetch("/api/admin/mesh/status", {
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});
	await throwIfNotOk(response);
	return AdminMeshStatusSchema.parse(await response.json());
}

export async function runAdminMeshProbes(
	adminToken: string,
	nodeIds: string[] = [],
): Promise<{ accepted_node_ids: string[]; revision: number }> {
	const response = await fetch("/api/admin/mesh/probes", {
		method: "POST",
		headers: {
			Accept: "application/json",
			"Content-Type": "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		body: JSON.stringify({ node_ids: nodeIds }),
	});
	await throwIfNotOk(response);
	return z
		.object({ accepted_node_ids: z.array(z.string()), revision: z.number() })
		.parse(await response.json());
}
