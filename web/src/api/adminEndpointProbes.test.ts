import { describe, expect, it } from "vitest";

import { AdminEndpointProbeHistoryResponseSchema } from "./adminEndpointProbes";

describe("AdminEndpointProbeHistoryResponseSchema", () => {
	it("accepts legacy probe history payloads without participating_nodes", () => {
		const parsed = AdminEndpointProbeHistoryResponseSchema.parse({
			endpoint_id: "endpoint-1",
			expected_nodes: 2,
			slots: [
				{
					hour: "2026-03-11T11:00:00Z",
					status: "up",
					ok_count: 2,
					sample_count: 2,
					skipped_count: 0,
					tested_count: 2,
					latency_ms_p50: 111,
					latency_ms_p95: 123,
					by_node: [],
				},
			],
		});

		expect(parsed.expected_nodes).toBe(2);
		expect(parsed.participating_nodes).toBeUndefined();
		expect(parsed.slots[0]?.participating_nodes).toBeUndefined();
	});
});
