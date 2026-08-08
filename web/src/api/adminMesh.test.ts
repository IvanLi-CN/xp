import { describe, expect, it } from "vitest";

import { AdminMeshPeerSchema } from "./adminMesh";

function peerFixture() {
	return {
		node_id: "peer-a",
		node_name: "alpha",
		api_base_url: "https://alpha.example.test",
		mesh_url: "https://alpha.example.test:443",
		mesh_capability: "enabled",
		mesh_reason: "mesh_available",
		current_path: "mesh",
		quality: "good",
		stale: false,
		breaker: "closed",
		last_sample_at: "2026-08-08T10:00:00Z",
		last_transition_at: "2026-08-08T09:00:00Z",
		availability_1h: 1,
		availability_24h: 1,
		mesh_availability_24h: 1,
		latency_p50_ms: 20,
		latency_p95_ms: 30,
		buckets: [
			{
				minute: "2026-08-08T10:00:00Z",
				mesh_success: 1,
				mesh_failure: 0,
				public_success: 0,
				public_failure: 0,
				fallback_success: 0,
				end_to_end_success: 1,
				end_to_end_failure: 0,
				latency_samples_ms: [20],
			},
		],
	};
}

describe("AdminMeshPeerSchema", () => {
	it("accepts legacy peers and defaults additive bucket counters", () => {
		const parsed = AdminMeshPeerSchema.parse(peerFixture());

		expect(parsed.mesh_transport).toBeUndefined();
		expect(parsed.buckets[0]?.mesh_h2_requests).toBe(0);
		expect(parsed.buckets[0]?.mesh_connection_starts).toBe(0);
	});

	it("parses bounded Mesh transport reuse evidence", () => {
		const parsed = AdminMeshPeerSchema.parse({
			...peerFixture(),
			mesh_transport: {
				protocol: "h2",
				health: "healthy",
				connection_generation: 4,
				current_connection_requests: 32,
				requests_5m: 32,
				connection_starts_5m: 1,
				requests_1h: 400,
				connection_starts_1h: 2,
				last_connection_started_at: "2026-08-08T09:58:00Z",
			},
		});

		expect(parsed.mesh_transport?.protocol).toBe("h2");
		expect(parsed.mesh_transport?.connection_starts_5m).toBe(1);
	});
});
