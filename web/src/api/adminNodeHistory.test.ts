import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import { AdminNodeHistoryResponseSchema } from "./adminNodeHistory";

describe("admin node history schemas", () => {
	it("accepts node history payload", () => {
		const parsed = AdminNodeHistoryResponseSchema.parse({
			node: {
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
				access_host: fixtureCatalog.host.primary(),
				quota_limit_bytes: 0,
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: null,
				},
			},
			history: {
				node_id: fixtureCatalog.identifier.nodePrimary(),
				last_synced_at: fixtureCatalog.timestamp.baseline(),
				last_sync_error: null,
				daily_traffic: [
					{
						date: "2026-05-20",
						uplink_bytes: fixtureCatalog.metric.trafficBytes(),
						downlink_bytes: fixtureCatalog.metric.trafficBytes(),
						updated_at: fixtureCatalog.timestamp.baseline(),
					},
				],
				daily_component_status: [
					{
						date: "2026-05-20",
						components: [
							{
								component: "xray",
								status: "down",
								observed_at: fixtureCatalog.timestamp.baseline(),
							},
						],
					},
				],
				component_status_events: [
					{
						event_id: "evt-1",
						occurred_at: fixtureCatalog.timestamp.baseline(),
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
