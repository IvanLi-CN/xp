import { describe, expect, it } from "vitest";

import type { AdminNodeIpUsageResponse } from "../api/adminIpUsage";
import type { AdminNodeTcpConnectionsResponse } from "../api/adminTcpConnections";
import type { AdminNodeTrafficResponse } from "../api/adminTraffic";
import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	alignNodeIpUsageResponse,
	alignNodeTcpConnectionsResponse,
	alignNodeTrafficResponse,
	emptyNodeTrafficResponse,
} from "./timeWindowReports";

const node = {
	node_id: fixtureCatalog.identifier.nodePrimary(),
	node_name: fixtureCatalog.identifier.nodeNamePrimary(),
	access_host: fixtureCatalog.host.primary(),
	api_base_url: fixtureCatalog.url.primaryApi(),
	quota_limit_bytes: 0,
	quota_reset: {
		policy: "monthly" as const,
		day_of_month: 1,
		tz_offset_minutes: null,
	},
};

const trafficResponse: AdminNodeTrafficResponse = {
	node,
	traffic: {
		window: "24h",
		window_start_at: fixtureCatalog.slotString.s628(),
		window_end_at: fixtureCatalog.slotString.s629(),
		timezone: "UTC",
		summary: {
			mode: "cycle",
			uplink_bytes: fixtureCatalog.slotNumber.n24(),
			downlink_bytes: fixtureCatalog.slotNumber.n19(),
			total_bytes: fixtureCatalog.slotNumber.n25(),
			complete: true,
		},
		current: [
			{
				start_at: fixtureCatalog.slotString.s630(),
				end_at: fixtureCatalog.slotString.s629(),
				uplink_bytes: fixtureCatalog.slotNumber.n3(),
				downlink_bytes: fixtureCatalog.slotNumber.n5(),
				total_bytes: fixtureCatalog.slotNumber.n24(),
				complete: true,
				is_current_day: false,
			},
		],
		reference: [],
		partial: false,
		last_sample_at: fixtureCatalog.slotString.s629(),
		warnings: [],
	},
};

describe("time window report alignment", () => {
	it("keeps traffic timestamps fixed and leaves the current tail blank", () => {
		const aligned = alignNodeTrafficResponse(
			trafficResponse,
			"24h",
			Date.parse("2026-08-08T12:07:30Z"),
		);

		expect(aligned.traffic.window_end_at).toBe("2026-08-08T12:05:00.000Z");
		expect(aligned.traffic.current).toHaveLength(288);
		expect(aligned.traffic.current.at(-2)?.start_at).toBe(
			"2026-08-08T11:55:00Z",
		);
		expect(aligned.traffic.current.at(-2)?.total_bytes).toBe(10);
		expect(aligned.traffic.current.at(-1)?.start_at).toBe(
			"2026-08-08T12:00:00.000Z",
		);
		expect(aligned.traffic.current.at(-1)?.total_bytes).toBeNull();
	});

	it("builds a target-axis traffic report without stale summary values", () => {
		const empty = emptyNodeTrafficResponse(
			trafficResponse,
			"24h",
			Date.parse("2026-08-08T12:07:30Z"),
		);

		expect(empty.traffic.summary.total_bytes).toBe(0);
		expect(empty.traffic.summary.complete).toBe(false);
		expect(empty.traffic.last_sample_at).toBeNull();
		expect(
			empty.traffic.current.every((point) => point.total_bytes === null),
		).toBe(true);
	});

	it("aligns minute reports to now without moving cached samples", () => {
		const report: AdminNodeTcpConnectionsResponse = {
			node,
			window: "24h",
			window_start: fixtureCatalog.slotString.s628(),
			window_end: fixtureCatalog.slotString.s629(),
			warnings: [],
			endpoints: [
				{
					endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
					endpoint_tag: fixtureCatalog.identifier.endpointTagPrimary(),
					port: 443,
				},
			],
			per_endpoint_series: [
				{
					endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
					endpoint_tag: fixtureCatalog.identifier.endpointTagPrimary(),
					port: 443,
					series: [
						{ minute: fixtureCatalog.slotString.s629(), count: 3 },
						{ minute: fixtureCatalog.slotString.s634(), count: 9 },
					],
				},
			],
		};

		const aligned = alignNodeTcpConnectionsResponse(
			report,
			"24h",
			Date.parse("2026-08-08T12:07:30Z"),
		);

		expect(aligned.window_end).toBe("2026-08-08T12:07:00.000Z");
		expect(aligned.per_endpoint_series[0]?.series).toEqual([
			{ minute: fixtureCatalog.slotString.s629(), count: 3 },
		]);
	});

	it("clears IP rows when timeline clipping makes exact aggregates impossible", () => {
		const report: AdminNodeIpUsageResponse = {
			node,
			window: "24h",
			geo_source: "country_is",
			window_start: fixtureCatalog.slotString.s635(),
			window_end: fixtureCatalog.slotString.s628(),
			warnings: [],
			unique_ip_series: [],
			timeline: [
				{
					lane_key: "edge-a::203.0.113.1",
					endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
					endpoint_tag: fixtureCatalog.identifier.endpointTagPrimary(),
					ip: fixtureCatalog.address.primaryIpv4(),
					minutes: 1_440,
					segments: [
						{
							start_minute: fixtureCatalog.slotString.s636(),
							end_minute: fixtureCatalog.slotString.s637(),
						},
					],
				},
			],
			ips: [
				{
					ip: fixtureCatalog.address.primaryIpv4(),
					minutes: 1_440,
					endpoint_tags: ["edge-a", "aged-out"],
					region: "Test",
					operator: "Example",
					last_seen_at: fixtureCatalog.slotString.s637(),
				},
			],
		};

		const aligned = alignNodeIpUsageResponse(
			report,
			"24h",
			Date.parse("2026-08-08T12:00:30Z"),
		);

		expect(aligned.ips).toEqual([]);

		const completeIps = [
			...report.ips,
			{
				...report.ips[0],
				ip: fixtureCatalog.address.secondaryIpv4(),
				endpoint_tags: ["outside-top-20"],
			},
		];
		const unchanged = alignNodeIpUsageResponse(
			{
				...report,
				window_start: fixtureCatalog.slotString.s639(),
				window_end: fixtureCatalog.slotString.s629(),
				ips: completeIps,
			},
			"24h",
			Date.parse("2026-08-08T12:00:30Z"),
		);

		expect(unchanged.ips).toEqual(completeIps);
	});
});
