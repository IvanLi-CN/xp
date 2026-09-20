import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	AdminMeshConnectionUsageSchema,
	AdminMeshPeerSchema,
} from "./adminMesh";

function peerFixture() {
	return {
		node_id: fixtureCatalog.identifier.nodePrimary(),
		node_name: fixtureCatalog.identifier.nodeNamePrimary(),
		api_base_url: fixtureCatalog.url.primaryApi(),
		mesh_url: fixtureCatalog.url.primaryApi(),
		mesh_capability: "enabled",
		mesh_reason: "mesh_available",
		current_path: "mesh",
		quality: "good",
		stale: false,
		breaker: "closed",
		last_sample_at: fixtureCatalog.timestamp.recent(),
		last_transition_at: fixtureCatalog.timestamp.baseline(),
		availability_1h: fixtureCatalog.metric.availabilityFull(),
		availability_24h: fixtureCatalog.metric.availabilityFull(),
		mesh_availability_24h: fixtureCatalog.metric.availabilityFull(),
		latency_p50_ms: fixtureCatalog.number.value20(),
		latency_p95_ms: fixtureCatalog.number.value30(),
		buckets: [
			{
				minute: fixtureCatalog.timestamp.recent(),
				mesh_success: 1,
				mesh_failure: 0,
				public_success: 0,
				public_failure: 0,
				fallback_success: 0,
				end_to_end_success: 1,
				end_to_end_failure: 0,
				latency_samples_ms: [fixtureCatalog.number.value20()],
			},
		],
	};
}

describe("AdminMeshPeerSchema", () => {
	it("keeps unavailable socket inspection distinct from zero traffic", () => {
		const parsed = AdminMeshConnectionUsageSchema.parse({
			supported: false,
			sampled_at: null,
			warning: "socket inspection unavailable",
			user_inbound: {
				connections: null,
				external: null,
				cluster_peer: null,
				unknown: null,
				sources: [],
				sources_truncated: false,
			},
		});

		expect(parsed.user_inbound.external).toBeNull();
	});

	it("accepts legacy peers and defaults additive bucket counters", () => {
		const parsed = AdminMeshPeerSchema.parse(peerFixture());

		expect(parsed.mesh_transport).toBeUndefined();
		expect(parsed.buckets[0]?.mesh_h2_requests).toBe(0);
		expect(parsed.buckets[0]?.mesh_connection_starts).toBe(0);
	});

	it("accepts a persisted reverse route without new assignment fields", () => {
		const parsed = AdminMeshPeerSchema.parse({
			...peerFixture(),
			active_route: {
				kind: "reverse_relay",
				rendezvous: "node-rendezvous-a",
				generation: fixtureCatalog.number.value7(),
				readiness: "active",
			},
		});

		expect(parsed.active_route?.rendezvous_role).toBeUndefined();
	});

	it("parses bounded Mesh transport reuse evidence", () => {
		const parsed = AdminMeshPeerSchema.parse({
			...peerFixture(),
			mesh_transport: {
				protocol: "h2",
				health: "healthy",
				connection_generation: fixtureCatalog.number.value4(),
				current_connection_requests: fixtureCatalog.number.value32(),
				requests_5m: fixtureCatalog.number.value32(),
				connection_starts_5m: fixtureCatalog.number.value1(),
				requests_1h: fixtureCatalog.number.value200(),
				connection_starts_1h: fixtureCatalog.number.value2(),
				last_connection_started_at: fixtureCatalog.timestamp.recent(),
			},
		});

		expect(parsed.mesh_transport?.protocol).toBe("h2");
		expect(parsed.mesh_transport?.connection_starts_5m).toBe(1);
	});

	it("parses separate Reverse underlay and user inbound evidence", () => {
		const parsed = AdminMeshPeerSchema.parse({
			...peerFixture(),
			reverse_underlay: {
				logical_links: 1,
				physical_connections: 3,
				limit_per_link: 2,
				state: "over_limit",
				links: [
					{
						target_node_id: "node-target",
						rendezvous_node_id: "node-rendezvous",
						role: "primary",
						generation: fixtureCatalog.number.value4(),
						connections: 3,
						limit: 2,
						state: "over_limit",
					},
				],
			},
		});

		expect(parsed.reverse_underlay?.physical_connections).toBe(3);
		expect(parsed.reverse_underlay?.links[0]?.state).toBe("over_limit");
	});

	it.each(["primary", "standby", "bootstrap"] as const)(
		"accepts the %s reverse rendezvous role",
		(rendezvous_role) => {
			const parsed = AdminMeshPeerSchema.parse({
				...peerFixture(),
				active_route: {
					kind: "reverse_relay",
					rendezvous: "node-rendezvous-a",
					rendezvous_role,
					primary_rendezvous: "node-rendezvous-a",
					standby_rendezvous: "node-rendezvous-b",
					generation: fixtureCatalog.number.value7(),
					readiness: "active",
				},
			});

			expect(parsed.active_route?.rendezvous_role).toBe(rendezvous_role);
		},
	);
});
