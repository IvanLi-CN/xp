import { describe, expect, it } from "vitest";

import { AdminNodeHistoryResponseSchema } from "./adminNodeHistory";

describe("admin node history schemas", () => {
	it("accepts node history payload", () => {
		const parsed = AdminNodeHistoryResponseSchema.parse({
			node: {
				node_id: "node-1",
				node_name: "node-1",
				api_base_url: "https://node-1.example.com",
				access_host: "node-1.example.com",
				quota_limit_bytes: 0,
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: null,
				},
			},
			history: {
				node_id: "node-1",
				last_synced_at: "2026-05-20T08:00:00Z",
				last_sync_error: null,
				daily_traffic: [
					{
						date: "2026-05-20",
						uplink_bytes: 1024,
						downlink_bytes: 2048,
						updated_at: "2026-05-20T08:00:00Z",
					},
				],
				daily_component_status: [
					{
						date: "2026-05-20",
						components: [
							{
								component: "xray",
								status: "down",
								observed_at: "2026-05-20T08:00:00Z",
							},
						],
					},
				],
				component_status_events: [
					{
						event_id: "evt-1",
						occurred_at: "2026-05-20T07:50:00Z",
						component: "xray",
						message: "xray status changed: up -> down",
						from_status: "up",
						to_status: "down",
					},
				],
			},
		});

		expect(parsed.history?.daily_traffic[0]?.uplink_bytes).toBe(1024);
		expect(parsed.history?.component_status_events[0]?.component).toBe("xray");
	});
});
