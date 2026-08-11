import { describe, expect, it } from "vitest";

import type { AdminNodeIpUsageResponse } from "../api/adminIpUsage";
import type { AdminNodeTcpConnectionsResponse } from "../api/adminTcpConnections";
import type { AdminNodeTrafficResponse } from "../api/adminTraffic";
import type { NodeQuotaReset } from "../api/quotaReset";
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
	quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
	quota_reset: fixtureCatalog.quota.reset() as NodeQuotaReset,
};

const trafficResponse: AdminNodeTrafficResponse = {
	node,
	traffic: {
		window: "24h",
		window_start_at: fixtureCatalog.timestamp.t20260807T120000(),
		window_end_at: fixtureCatalog.timestamp.t20260808T120000(),
		timezone: "UTC",
		summary: {
			mode: "cycle",
			uplink_bytes: fixtureCatalog.number.value10(),
			downlink_bytes: fixtureCatalog.number.value20(),
			total_bytes: fixtureCatalog.number.value30(),
			complete: true,
		},
		current: [
			{
				start_at: fixtureCatalog.timestamp.t20260808T115500(),
				end_at: fixtureCatalog.timestamp.t20260808T120000(),
				uplink_bytes: fixtureCatalog.number.value4(),
				downlink_bytes: fixtureCatalog.number.value6(),
				total_bytes: fixtureCatalog.number.value10(),
				complete: true,
				is_current_day: false,
			},
		],
		reference: [],
		partial: false,
		last_sample_at: fixtureCatalog.timestamp.t20260808T120000(),
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
			window_start: fixtureCatalog.timestamp.t20260807T120000(),
			window_end: fixtureCatalog.timestamp.t20260808T120000(),
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
						{ minute: fixtureCatalog.timestamp.t20260808T120000(), count: 3 },
						{ minute: fixtureCatalog.timestamp.t20260808T120800(), count: 9 },
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
			{ minute: fixtureCatalog.timestamp.t20260808T120000(), count: 3 },
		]);
	});

	it("clears IP rows when timeline clipping makes exact aggregates impossible", () => {
		const report: AdminNodeIpUsageResponse = {
			node,
			window: "24h",
			geo_source: "country_is",
			window_start: fixtureCatalog.timestamp.t20260806T120000(),
			window_end: fixtureCatalog.timestamp.t20260807T120000(),
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
							start_minute: fixtureCatalog.timestamp.t20260807T115800(),
							end_minute: fixtureCatalog.timestamp.t20260807T120200(),
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
					last_seen_at: fixtureCatalog.timestamp.t20260807T120200(),
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
				window_start: fixtureCatalog.timestamp.t20260807T120100(),
				window_end: fixtureCatalog.timestamp.t20260808T120000(),
				ips: completeIps,
			},
			"24h",
			Date.parse("2026-08-08T12:00:30Z"),
		);

		expect(unchanged.ips).toEqual(completeIps);
	});
});
