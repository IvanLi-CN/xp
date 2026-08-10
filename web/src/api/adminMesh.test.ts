import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import { AdminMeshPeerSchema } from "./adminMesh";

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
});
