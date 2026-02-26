import { describe, expect, it } from "vitest";

import {
	AdminNodeRuntimeDetailResponseSchema,
	AdminNodesRuntimeResponseSchema,
} from "./adminNodeRuntime";

describe("admin node runtime schemas", () => {
	it("accepts runtime list payload", () => {
		const parsed = AdminNodesRuntimeResponseSchema.parse({
			partial: false,
			unreachable_nodes: [],
			items: [
				{
					node_id: "node-1",
					node_name: "node-1",
					api_base_url: "https://node-1.example.com",
					access_host: "node-1.example.com",
					summary: {
						status: "up",
						updated_at: "2026-02-26T08:00:00Z",
					},
					components: [
						{
							component: "xp",
							status: "up",
							last_ok_at: null,
							last_fail_at: null,
							down_since: null,
							consecutive_failures: 0,
							recoveries_observed: 0,
							restart_attempts: 0,
							last_restart_at: null,
							last_restart_fail_at: null,
						},
					],
					recent_slots: [
						{
							slot_start: "2026-02-26T07:30:00Z",
							status: "up",
						},
					],
				},
			],
		});

		expect(parsed.items).toHaveLength(1);
		expect(parsed.items[0]?.summary.status).toBe("up");
	});

	it("accepts runtime detail payload", () => {
		const parsed = AdminNodeRuntimeDetailResponseSchema.parse({
			node: {
				node_id: "node-1",
				node_name: "node-1",
				api_base_url: "https://node-1.example.com",
				access_host: "node-1.example.com",
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: null,
				},
			},
			summary: {
				status: "degraded",
				updated_at: "2026-02-26T08:00:00Z",
			},
			components: [
				{
					component: "xray",
					status: "down",
					last_ok_at: null,
					last_fail_at: "2026-02-26T07:59:50Z",
					down_since: "2026-02-26T07:59:50Z",
					consecutive_failures: 3,
					recoveries_observed: 1,
					restart_attempts: 2,
					last_restart_at: "2026-02-26T07:59:55Z",
					last_restart_fail_at: null,
				},
			],
			recent_slots: [
				{
					slot_start: "2026-02-26T07:30:00Z",
					status: "degraded",
				},
			],
			events: [
				{
					event_id: "evt-1",
					occurred_at: "2026-02-26T07:59:55Z",
					component: "xray",
					kind: "restart_requested",
					message: "xray restart requested",
					from_status: null,
					to_status: "down",
				},
			],
		});

		expect(parsed.events[0]?.kind).toBe("restart_requested");
	});
});
