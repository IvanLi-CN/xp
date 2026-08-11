import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	AdminNodeRuntimeDetailResponseSchema,
	AdminNodesRuntimeResponseSchema,
} from "./adminNodeRuntime";

describe("admin node runtime schemas", () => {
	it("requires the server completeness fields", () => {
		expect(() =>
			AdminNodesRuntimeResponseSchema.parse({ items: [] }),
		).toThrow();
	});

	it("accepts runtime list payload", () => {
		const parsed = AdminNodesRuntimeResponseSchema.parse({
			partial: false,
			unreachable_nodes: [],
			items: [
				{
					node_id: fixtureCatalog.identifier.nodePrimary(),
					node_name: fixtureCatalog.identifier.nodeNamePrimary(),
					api_base_url: fixtureCatalog.url.primaryApi(),
					access_host: fixtureCatalog.host.primary(),
					summary: {
						status: "up",
						updated_at: fixtureCatalog.timestamp.baseline(),
					},
					components: [
						{
							component: "xp",
							status: "up",
							last_ok_at: fixtureCatalog.timestamp.none(),
							last_fail_at: fixtureCatalog.timestamp.none(),
							down_since: fixtureCatalog.timestamp.none(),
							consecutive_failures: 0,
							recoveries_observed: 0,
							restart_attempts: 0,
							last_restart_at: fixtureCatalog.timestamp.none(),
							last_restart_fail_at: fixtureCatalog.timestamp.none(),
						},
					],
					recent_slots: [
						{
							slot_start: fixtureCatalog.timestamp.baseline(),
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
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
				access_host: fixtureCatalog.host.primary(),
				quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
				quota_reset: fixtureCatalog.quota.reset(),
			},
			summary: {
				status: "degraded",
				updated_at: fixtureCatalog.timestamp.baseline(),
			},
			components: [
				{
					component: "xray",
					status: "down",
					last_ok_at: fixtureCatalog.timestamp.none(),
					last_fail_at: fixtureCatalog.timestamp.baseline(),
					down_since: fixtureCatalog.timestamp.baseline(),
					consecutive_failures: 3,
					recoveries_observed: 1,
					restart_attempts: 2,
					last_restart_at: fixtureCatalog.timestamp.baseline(),
					last_restart_fail_at: fixtureCatalog.timestamp.none(),
				},
			],
			recent_slots: [
				{
					slot_start: fixtureCatalog.timestamp.baseline(),
					status: "degraded",
				},
			],
			events: [
				{
					event_id: "evt-1",
					occurred_at: fixtureCatalog.timestamp.baseline(),
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
