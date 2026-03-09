import { describe, expect, it } from "vitest";

import {
	AdminIpGeoDbResponseSchema,
	AdminIpGeoDbUpdateResponseSchema,
} from "./adminIpGeoDb";

describe("admin IP geo DB schemas", () => {
	it("accepts cluster geo DB status payload", () => {
		const parsed = AdminIpGeoDbResponseSchema.parse({
			settings: {
				provider: "dbip_lite",
				auto_update_enabled: true,
				update_interval_days: 7,
			},
			partial: true,
			unreachable_nodes: ["node-2"],
			nodes: [
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
					mode: "managed",
					running: true,
					city_db_path: "/var/lib/xp/geoip/dbip-city-lite.mmdb",
					asn_db_path: "/var/lib/xp/geoip/dbip-asn-lite.mmdb",
					last_started_at: "2026-03-09T08:00:00Z",
					last_success_at: "2026-03-08T08:00:00Z",
					next_scheduled_at: null,
					last_error: null,
				},
			],
		});

		expect(parsed.nodes[0]?.mode).toBe("managed");
		expect(parsed.settings.provider).toBe("dbip_lite");
	});

	it("accepts manual update result payload", () => {
		const parsed = AdminIpGeoDbUpdateResponseSchema.parse({
			partial: false,
			unreachable_nodes: [],
			nodes: [
				{
					node_id: "node-1",
					status: "accepted",
					message: null,
				},
				{
					node_id: "node-2",
					status: "skipped",
					message: "external override is active",
				},
			],
		});

		expect(parsed.nodes[1]?.status).toBe("skipped");
	});
});
