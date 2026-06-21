import type { AdminNode } from "../api/adminNodes";
import type {
	AdminNodeTcpConnectionsResponse,
	AdminTcpConnectionUsageWarning,
	AdminTcpConnectionUsageWindow,
} from "../api/adminTcpConnections";

export type WindowedNodeTcpConnectionStories = Record<
	AdminTcpConnectionUsageWindow,
	AdminNodeTcpConnectionsResponse
>;

const ONE_MINUTE_MS = 60_000;

type EndpointSeed = {
	endpointId: string;
	endpointTag: string;
	port: number;
	ranges: ReadonlyArray<
		readonly [startMinute: number, endMinute: number, count: number]
	>;
};

function isoMinuteFromOffset(
	startMinuteIso: string,
	offsetMinutes: number,
): string {
	const baseMs = new Date(startMinuteIso).getTime();
	return new Date(baseMs + offsetMinutes * ONE_MINUTE_MS).toISOString();
}

function buildSeries(
	window: AdminTcpConnectionUsageWindow,
	windowStart: string,
	seeds: EndpointSeed[],
) {
	const totalMinutes = window === "24h" ? 24 * 60 : 7 * 24 * 60;
	return seeds.map((seed) => {
		const perMinute = new Array<number>(totalMinutes).fill(0);
		for (const [startMinute, endMinute, count] of seed.ranges) {
			for (let minute = startMinute; minute <= endMinute; minute += 1) {
				if (minute < 0 || minute >= totalMinutes) continue;
				perMinute[minute] = count;
			}
		}
		return {
			endpoint_id: seed.endpointId,
			endpoint_tag: seed.endpointTag,
			port: seed.port,
			series: perMinute.map((count, minute) => ({
				minute: isoMinuteFromOffset(windowStart, minute),
				count,
			})),
		};
	});
}

function buildWindowedReport(
	node: AdminNode,
	window: AdminTcpConnectionUsageWindow,
	windowStart: string,
	endpoints: EndpointSeed[],
	warnings: AdminTcpConnectionUsageWarning[] = [],
): AdminNodeTcpConnectionsResponse {
	return {
		node,
		window,
		window_start: windowStart,
		window_end: isoMinuteFromOffset(
			windowStart,
			(window === "24h" ? 24 * 60 : 7 * 24 * 60) - 1,
		),
		warnings,
		endpoints: endpoints.map((endpoint) => ({
			endpoint_id: endpoint.endpointId,
			endpoint_tag: endpoint.endpointTag,
			port: endpoint.port,
		})),
		per_endpoint_series: buildSeries(window, windowStart, endpoints),
	};
}

function build24hSeeds(node: AdminNode): EndpointSeed[] {
	const prefix = node.node_name || node.node_id;
	return [
		{
			endpointId: `${node.node_id}-endpoint-a`,
			endpointTag: `${prefix}-edge-a`,
			port: 443,
			ranges: [
				[30, 120, 3],
				[410, 580, 6],
				[1180, 1300, 4],
				[1380, 1439, 7],
			],
		},
		{
			endpointId: `${node.node_id}-endpoint-b`,
			endpointTag: `${prefix}-edge-b`,
			port: 8443,
			ranges: [
				[80, 160, 1],
				[600, 760, 2],
				[1320, 1439, 3],
			],
		},
		{
			endpointId: `${node.node_id}-endpoint-c`,
			endpointTag: `${prefix}-edge-c`,
			port: 9443,
			ranges: [
				[250, 300, 2],
				[900, 960, 5],
				[1260, 1360, 1],
			],
		},
	];
}

function build7dSeeds(node: AdminNode): EndpointSeed[] {
	const prefix = node.node_name || node.node_id;
	return [
		{
			endpointId: `${node.node_id}-endpoint-a`,
			endpointTag: `${prefix}-edge-a`,
			port: 443,
			ranges: [
				[120, 420, 4],
				[2200, 2460, 7],
				[6200, 6500, 5],
				[9700, 10079, 8],
			],
		},
		{
			endpointId: `${node.node_id}-endpoint-b`,
			endpointTag: `${prefix}-edge-b`,
			port: 8443,
			ranges: [
				[800, 980, 2],
				[3500, 3890, 3],
				[7100, 7420, 4],
				[9550, 10079, 2],
			],
		},
		{
			endpointId: `${node.node_id}-endpoint-c`,
			endpointTag: `${prefix}-edge-c`,
			port: 9443,
			ranges: [
				[1500, 1740, 1],
				[4700, 4880, 6],
				[8400, 8650, 2],
			],
		},
	];
}

export function buildDenseNodeTcpConnectionStories(
	node: AdminNode,
): WindowedNodeTcpConnectionStories {
	return {
		"24h": buildWindowedReport(
			node,
			"24h",
			"2026-03-07T01:00:00Z",
			build24hSeeds(node),
		),
		"7d": buildWindowedReport(
			node,
			"7d",
			"2026-03-01T01:00:00Z",
			build7dSeeds(node),
		),
	};
}
