import { describe, expect, it } from "vitest";

import {
	AdminNodeIpUsageResponseSchema,
	AdminUserIpUsageResponseSchema,
} from "./adminIpUsage";

describe("admin IP usage schemas", () => {
	it("accepts node IP usage payload", () => {
		const parsed = AdminNodeIpUsageResponseSchema.parse({
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
			geo_source: "managed_dbip_lite",
			window_start: "2026-03-07T01:00:00Z",
			window_end: "2026-03-08T00:59:00Z",
			warnings: [
				{
					code: "geo_db_missing",
					message:
						"IP geolocation DB is unavailable; region and operator fields will be empty.",
				},
			],
			unique_ip_series: [{ minute: "2026-03-08T00:59:00Z", count: 2 }],
			timeline: [
				{
					lane_key: "tokyo-vless::203.0.113.7",
					endpoint_id: "ep-1",
					endpoint_tag: "tokyo-vless",
					ip: "203.0.113.7",
					minutes: 2,
					segments: [
						{
							start_minute: "2026-03-08T00:58:00Z",
							end_minute: "2026-03-08T00:59:00Z",
						},
					],
				},
			],
			ips: [
				{
					ip: "203.0.113.7",
					minutes: 2,
					endpoint_tags: ["tokyo-vless"],
					region: "Japan / Tokyo",
					operator: "ExampleNet",
					last_seen_at: "2026-03-08T00:59:00Z",
				},
			],
		});

		expect(parsed.timeline[0]?.segments).toHaveLength(1);
	});

	it("accepts grouped user IP usage payload", () => {
		const parsed = AdminUserIpUsageResponseSchema.parse({
			user: {
				user_id: "user-1",
				display_name: "Alice",
			},
			window: "7d",
			partial: true,
			unreachable_nodes: ["node-2"],
			warnings: [
				{
					code: "online_stats_unavailable",
					message:
						"Xray online IP stats are unavailable; enable statsUserOnline to collect inbound IP usage.",
				},
			],
			groups: [
				{
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
					geo_source: "external_override",
					window_start: "2026-03-01T01:00:00Z",
					window_end: "2026-03-08T00:59:00Z",
					warnings: [],
					unique_ip_series: [{ minute: "2026-03-08T00:59:00Z", count: 1 }],
					timeline: [],
					ips: [],
				},
			],
		});

		expect(parsed.partial).toBe(true);
		expect(parsed.unreachable_nodes).toEqual(["node-2"]);
	});
});
