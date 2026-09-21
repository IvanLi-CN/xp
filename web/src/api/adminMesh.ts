import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const MeshTelemetryPathSchema = z.enum(["mesh", "public"]);
export const MeshActiveRouteKindSchema = z.enum([
	"reality_direct",
	"reverse_relay",
	"public",
]);
export const AdminMeshActiveRouteSchema = z.object({
	kind: MeshActiveRouteKindSchema,
	rendezvous: z.string().nullable().optional(),
	rendezvous_role: z
		.enum(["primary", "standby", "bootstrap"])
		.nullable()
		.optional(),
	primary_rendezvous: z.string().nullable().optional(),
	standby_rendezvous: z.string().nullable().optional(),
	generation: z.number().int().nonnegative().nullable().optional(),
	readiness: z.string().nullable().optional(),
});
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
export const MeshPeerReasonSchema = z.enum([
	"mesh_available",
	"missing_endpoint",
	"ambiguous_endpoint",
	"invalid_access_host",
	"no_sample",
	"transport_timeout",
	"transport_error",
	"protocol_rejected",
	"fallback_active",
]);
export const MeshEndpointTransportSchema = z.enum([
	"vision_tcp",
	"xhttp_reality_fallback",
]);
export const NativeReverseRelayStateSchema = z.enum([
	"disabled_pending_rework",
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
	mesh_h2_requests: z.number().int().nonnegative().default(0),
	mesh_connection_starts: z.number().int().nonnegative().default(0),
});

export const AdminMeshTransportSchema = z.object({
	protocol: z.enum(["h2", "other"]).nullable(),
	health: z.enum(["unknown", "healthy", "churning"]),
	connection_generation: z.number().int().nonnegative(),
	current_connection_requests: z.number().int().nonnegative(),
	requests_5m: z.number().int().nonnegative(),
	connection_starts_5m: z.number().int().nonnegative(),
	requests_1h: z.number().int().nonnegative(),
	connection_starts_1h: z.number().int().nonnegative(),
	last_connection_started_at: z.string().nullable(),
});

export const AdminConnectionClassificationSchema = z.enum([
	"cluster_peer",
	"external",
	"unknown",
]);
export const AdminConnectionSourceSchema = z.object({
	address: z.string(),
	connections: z.number().int().nonnegative(),
	classification: AdminConnectionClassificationSchema,
});
export const AdminUserInboundStatusSchema = z.object({
	connections: z.number().int().nonnegative().nullable(),
	external: z.number().int().nonnegative().nullable(),
	cluster_peer: z.number().int().nonnegative().nullable(),
	unknown: z.number().int().nonnegative().nullable(),
	sources: z.array(AdminConnectionSourceSchema),
	sources_truncated: z.boolean().default(false),
});
export const AdminMeshConnectionUsageSchema = z.object({
	supported: z.boolean(),
	sampled_at: z.string().nullable(),
	warning: z.string().nullable().optional(),
	user_inbound: AdminUserInboundStatusSchema,
});
export const AdminReverseUnderlayStateSchema = z.enum([
	"ok",
	"over_limit",
	"unknown",
	"unavailable",
]);
export const AdminReverseLinkStatusSchema = z.object({
	target_node_id: z.string(),
	rendezvous_node_id: z.string(),
	role: z.enum(["primary", "standby", "bootstrap"]),
	generation: z.number().int().nonnegative(),
	connections: z.number().int().nonnegative().nullable(),
	limit: z.number().int().positive(),
	state: AdminReverseUnderlayStateSchema,
});
export const AdminReverseUnderlayStatusSchema = z.object({
	logical_links: z.number().int().nonnegative(),
	physical_connections: z.number().int().nonnegative().nullable(),
	limit_per_link: z.number().int().positive(),
	state: AdminReverseUnderlayStateSchema,
	links: z.array(AdminReverseLinkStatusSchema),
});

export const AdminMeshPeerSchema = z.object({
	node_id: z.string(),
	node_name: z.string(),
	api_base_url: z.string(),
	mesh_url: z.string().nullable(),
	endpoint_transport: MeshEndpointTransportSchema.nullable().optional(),
	mesh_capability: z.enum(["enabled", "disabled"]).optional(),
	mesh_reason: MeshPeerReasonSchema.optional(),
	reverse_relay_state: NativeReverseRelayStateSchema.nullable().optional(),
	current_path: MeshTelemetryPathSchema.nullable(),
	active_route: AdminMeshActiveRouteSchema.optional(),
	quality: MeshQualitySchema,
	stale: z.boolean(),
	breaker: MeshBreakerStateSchema,
	direct_validation: z
		.enum([
			"configured_unverified",
			"verified",
			"transport_failed",
			"protocol_rejected",
		])
		.optional(),
	public_circuit: MeshBreakerStateSchema.optional(),
	last_sample_at: z.string().nullable(),
	last_transition_at: z.string().nullable(),
	availability_1h: z.number().nullable(),
	availability_24h: z.number().nullable(),
	mesh_availability_24h: z.number().nullable(),
	latency_p50_ms: z.number().nullable(),
	latency_p95_ms: z.number().nullable(),
	mesh_transport: AdminMeshTransportSchema.optional(),
	reverse_underlay: AdminReverseUnderlayStatusSchema.optional(),
	buckets: z.array(AdminMeshBucketSchema),
});

export const AdminMeshStatusSchema = z.object({
	generated_at: z.string(),
	revision: z.number(),
	cluster_mesh_enabled: z.boolean().optional(),
	local: z.object({
		node_id: z.string(),
		node_name: z.string(),
		cluster_id: z.string(),
		role: z.enum(["leader", "follower"]),
		leader_api_base_url: z.string(),
		term: z.number(),
		canary: z.object({
			enabled: z.boolean(),
			bind: z.string().nullable().optional(),
			acme_directory_url: z.string().nullable().optional(),
			cert_not_after: z.string().nullable().optional(),
			last_renewed_at: z.string().nullable().optional(),
			last_error: z.string().nullable().optional(),
		}),
		connection_usage: AdminMeshConnectionUsageSchema.optional(),
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

export async function updateAdminMeshConfig(
	adminToken: string,
	enabled: boolean,
): Promise<{ enabled: boolean }> {
	const response = await fetch("/api/admin/mesh/config", {
		method: "PUT",
		headers: {
			Accept: "application/json",
			"Content-Type": "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		body: JSON.stringify({ enabled }),
	});
	await throwIfNotOk(response);
	return z.object({ enabled: z.boolean() }).parse(await response.json());
}
