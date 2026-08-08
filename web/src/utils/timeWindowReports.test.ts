import { describe, expect, it } from "vitest";

import type { AdminNodeTcpConnectionsResponse } from "../api/adminTcpConnections";
import type { AdminNodeTrafficResponse } from "../api/adminTraffic";
import {
	alignNodeTcpConnectionsResponse,
	alignNodeTrafficResponse,
	emptyNodeTrafficResponse,
} from "./timeWindowReports";

const node = {
	node_id: "node-1",
	node_name: "Tokyo",
	access_host: "tokyo.example.com",
	api_base_url: "https://tokyo.example.com",
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
		window_start_at: "2026-08-07T12:00:00Z",
		window_end_at: "2026-08-08T12:00:00Z",
		timezone: "UTC",
		summary: {
			mode: "cycle",
			uplink_bytes: 10,
			downlink_bytes: 20,
			total_bytes: 30,
			complete: true,
		},
		current: [
			{
				start_at: "2026-08-08T11:55:00Z",
				end_at: "2026-08-08T12:00:00Z",
				uplink_bytes: 4,
				downlink_bytes: 6,
				total_bytes: 10,
				complete: true,
				is_current_day: false,
			},
		],
		reference: [],
		partial: false,
		last_sample_at: "2026-08-08T12:00:00Z",
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
			window_start: "2026-08-07T12:00:00Z",
			window_end: "2026-08-08T12:00:00Z",
			warnings: [],
			endpoints: [
				{ endpoint_id: "endpoint-1", endpoint_tag: "edge", port: 443 },
			],
			per_endpoint_series: [
				{
					endpoint_id: "endpoint-1",
					endpoint_tag: "edge",
					port: 443,
					series: [
						{ minute: "2026-08-08T12:00:00Z", count: 3 },
						{ minute: "2026-08-08T12:08:00Z", count: 9 },
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
			{ minute: "2026-08-08T12:00:00Z", count: 3 },
		]);
	});
});
