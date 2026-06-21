import { describe, expect, it } from "vitest";

import { AdminNodeTcpConnectionsResponseSchema } from "./adminTcpConnections";

describe("admin TCP connection schemas", () => {
	it("accepts node TCP connection usage payload", () => {
		const parsed = AdminNodeTcpConnectionsResponseSchema.parse({
			node: {
				node_id: "node-1",
				node_name: "Tokyo",
				api_base_url: "https://tokyo.example.com",
				access_host: "tokyo.example.com",
				quota_limit_bytes: 0,
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: null,
				},
			},
			window: "24h",
			window_start: "2026-03-07T01:00:00Z",
			window_end: "2026-03-08T00:59:00Z",
			warnings: [],
			endpoints: [
				{
					endpoint_id: "ep-1",
					endpoint_tag: "tokyo-vless",
					port: 443,
				},
			],
			per_endpoint_series: [
				{
					endpoint_id: "ep-1",
					endpoint_tag: "tokyo-vless",
					port: 443,
					series: [
						{
							minute: "2026-03-08T00:58:00Z",
							count: 2,
						},
						{
							minute: "2026-03-08T00:59:00Z",
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
