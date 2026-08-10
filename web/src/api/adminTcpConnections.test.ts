import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import { AdminNodeTcpConnectionsResponseSchema } from "./adminTcpConnections";

describe("admin TCP connection schemas", () => {
	it("accepts node TCP connection usage payload", () => {
		const parsed = AdminNodeTcpConnectionsResponseSchema.parse({
			node: {
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
				access_host: fixtureCatalog.host.primary(),
				quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
				quota_reset: fixtureCatalog.quota.reset(),
			},
			window: "24h",
			window_start: fixtureCatalog.timestamp.baseline(),
			window_end: fixtureCatalog.timestamp.recent(),
			warnings: [],
			endpoints: [
				{
					endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
					endpoint_tag: fixtureCatalog.identifier.endpointTagPrimary(),
					port: fixtureCatalog.endpoint.port443(),
				},
			],
			per_endpoint_series: [
				{
					endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
					endpoint_tag: fixtureCatalog.identifier.endpointTagPrimary(),
					port: fixtureCatalog.endpoint.port443(),
					series: [
						{
							minute: fixtureCatalog.timestamp.baseline(),
							count: 2,
						},
						{
							minute: fixtureCatalog.timestamp.recent(),
							count: 3,
						},
					],
				},
			],
		});

		expect(parsed.per_endpoint_series[0]?.series).toHaveLength(2);
		expect(parsed.endpoints[0]?.port).toBe(443);
	});
});
