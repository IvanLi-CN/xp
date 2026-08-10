import type {
	AdminIpUsageWindow,
	AdminNodeIpUsageResponse,
	AdminUserIpUsageResponse,
} from "../api/adminIpUsage";
import type { AdminMeshPeer } from "../api/adminMesh";
import type { AdminNode } from "../api/adminNodes";
import type {
	AdminNodeTcpConnectionsResponse,
	AdminTcpConnectionUsageWindow,
} from "../api/adminTcpConnections";

import { fixtureCatalog } from "./catalog";

type EndpointSeed = {
	endpointId: string;
	endpointTag: string;
	port: number;
	ranges: ReadonlyArray<
		readonly [startMinute: number, endMinute: number, count: number]
	>;
};

const ONE_MINUTE_MS = 60_000;

const storyNode: AdminNode = {
	node_id: fixtureCatalog.identifier.nodePrimary(),
	node_name: fixtureCatalog.identifier.nodeNamePrimary(),
	access_host: fixtureCatalog.host.primary(),
	api_base_url: fixtureCatalog.url.primaryApi(),
	quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
	quota_reset: fixtureCatalog.quota.reset() as AdminNode["quota_reset"],
};

function minuteFromOffset(
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
				if (minute >= 0 && minute < totalMinutes) perMinute[minute] = count;
			}
		}
		return {
			endpoint_id: seed.endpointId,
			endpoint_tag: seed.endpointTag,
			port: seed.port,
			series: perMinute.map((count, minute) => ({
				minute: minuteFromOffset(windowStart, minute),
				count,
			})),
		};
	});
}

function report(
	window: AdminTcpConnectionUsageWindow,
	windowStart: string,
	seeds: EndpointSeed[],
): AdminNodeTcpConnectionsResponse {
	const totalMinutes = window === "24h" ? 24 * 60 : 7 * 24 * 60;
	return {
		node: structuredClone(storyNode),
		window,
		window_start: windowStart,
		window_end: minuteFromOffset(windowStart, totalMinutes - 1),
		warnings: [],
		endpoints: seeds.map((seed) => ({
			endpoint_id: seed.endpointId,
			endpoint_tag: seed.endpointTag,
			port: seed.port,
		})),
		per_endpoint_series: buildSeries(window, windowStart, seeds),
	};
}

const seeds24h: EndpointSeed[] = [
	{
		endpointId: fixtureCatalog.identifier.endpointPrimary(),
		endpointTag: fixtureCatalog.identifier.endpointTagPrimary(),
		port: fixtureCatalog.endpoint.port443(),
		ranges: [[30, 120, 3]],
	},
	{
		endpointId: fixtureCatalog.identifier.endpointSecondary(),
		endpointTag: fixtureCatalog.identifier.endpointTagSecondary(),
		port: fixtureCatalog.endpoint.port8443(),
		ranges: [[410, 580, 6]],
	},
	{
		endpointId: fixtureCatalog.identifier.endpointTertiary(),
		endpointTag: fixtureCatalog.identifier.endpointTagSecondary(),
		port: fixtureCatalog.endpoint.port9443(),
		ranges: [[1180, 1300, 4]],
	},
];

const seeds7d: EndpointSeed[] = [
	{
		endpointId: fixtureCatalog.identifier.endpointPrimary(),
		endpointTag: fixtureCatalog.identifier.endpointTagPrimary(),
		port: fixtureCatalog.endpoint.port443(),
		ranges: [[120, 420, 4]],
	},
	{
		endpointId: fixtureCatalog.identifier.endpointSecondary(),
		endpointTag: fixtureCatalog.identifier.endpointTagSecondary(),
		port: fixtureCatalog.endpoint.port8443(),
		ranges: [[2200, 2460, 7]],
	},
	{
		endpointId: fixtureCatalog.identifier.endpointTertiary(),
		endpointTag: fixtureCatalog.identifier.endpointTagSecondary(),
		port: fixtureCatalog.endpoint.port9443(),
		ranges: [[6200, 6500, 5]],
	},
];

function ipReport(_window: AdminIpUsageWindow) {
	return {
		window_start: fixtureCatalog.timestamp.baseline(),
		window_end: fixtureCatalog.timestamp.recent(),
		warnings: [],
		unique_ip_series: [
			{ minute: fixtureCatalog.timestamp.baseline(), count: 1 },
			{ minute: fixtureCatalog.timestamp.recent(), count: 1 },
		],
		timeline: [
			{
				lane_key: `${fixtureCatalog.identifier.endpointTagPrimary()}::${fixtureCatalog.address.secondaryIpv4()}`,
				endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
				endpoint_tag: fixtureCatalog.identifier.endpointTagPrimary(),
				ip: fixtureCatalog.address.secondaryIpv4(),
				minutes: 12,
				segments: [
					{
						start_minute: fixtureCatalog.timestamp.baseline(),
						end_minute: fixtureCatalog.timestamp.recent(),
					},
				],
			},
		],
		ips: [
			{
				ip: fixtureCatalog.address.secondaryIpv4(),
				minutes: 12,
				endpoint_tags: [fixtureCatalog.identifier.endpointTagPrimary()],
				region: "Fixture region",
				operator: "Fixture operator",
				last_seen_at: fixtureCatalog.timestamp.recent(),
			},
		],
	};
}

function nodeIpUsage(window: AdminIpUsageWindow): AdminNodeIpUsageResponse {
	return {
		node: structuredClone(storyNode),
		window,
		geo_source: "country_is",
		...ipReport(window),
	};
}

function userIpUsage(window: AdminIpUsageWindow): AdminUserIpUsageResponse {
	return {
		user: {
			user_id: fixtureCatalog.identifier.userPrimary(),
			display_name: "Fixture user",
		},
		window,
		partial: false,
		unreachable_nodes: [],
		warnings: [],
		groups: [
			{
				node: structuredClone(storyNode),
				geo_source: "country_is",
				...ipReport(window),
			},
		],
	};
}

function duplicateNameUserIpUsage(
	window: AdminIpUsageWindow,
): AdminUserIpUsageResponse {
	const base = ipReport(window);
	return {
		user: {
			user_id: fixtureCatalog.identifier.userPrimary(),
			display_name: "Fixture user",
		},
		window,
		partial: false,
		unreachable_nodes: [],
		warnings: [],
		groups: [
			{
				node: {
					...structuredClone(storyNode),
					node_id: fixtureCatalog.identifier.endpointPrimary(),
					node_name: fixtureCatalog.slotString.s640(),
					access_host: fixtureCatalog.host.primary(),
					api_base_url: fixtureCatalog.url.primaryApi(),
				},
				geo_source: "country_is",
				...base,
			},
			{
				node: {
					...structuredClone(storyNode),
					node_id: fixtureCatalog.identifier.endpointSecondary(),
					node_name: fixtureCatalog.slotString.s640(),
					access_host: fixtureCatalog.host.secondary(),
					api_base_url: fixtureCatalog.url.secondaryApi(),
				},
				geo_source: "country_is",
				...base,
			},
		],
	};
}

function fiftyMeshPeers(): AdminMeshPeer[] {
	return Array.from({ length: 50 }, () => ({
		node_id: fixtureCatalog.identifier.nextMeshPeerNodeId(),
		node_name: fixtureCatalog.slotString.s291(),
		api_base_url: fixtureCatalog.url.primaryApi(),
		mesh_url: fixtureCatalog.url.primaryApi(),
		mesh_capability: "enabled",
		mesh_reason: "mesh_available",
		current_path: "mesh",
		quality: "good",
		stale: false,
		breaker: "closed",
		last_sample_at: fixtureCatalog.timestamp.recent(),
		last_transition_at: fixtureCatalog.timestamp.baseline(),
		availability_1h: fixtureCatalog.metric.availabilityFull(),
		availability_24h: fixtureCatalog.metric.availabilityHigh(),
		mesh_availability_24h: fixtureCatalog.metric.availabilityHigh(),
		latency_p50_ms: fixtureCatalog.metric.latencyLow(),
		latency_p95_ms: fixtureCatalog.metric.latencyHigh(),
		buckets: [],
	}));
}

export const fixtureStoryData = {
	node: () => structuredClone(storyNode),
	nodeTcpConnections: () => ({
		"24h": report("24h", fixtureCatalog.timestamp.recent(), seeds24h),
		"7d": report("7d", fixtureCatalog.timestamp.baseline(), seeds7d),
	}),
	nodeIpUsage: () => ({
		"24h": nodeIpUsage("24h"),
		"7d": nodeIpUsage("7d"),
	}),
	userIpUsage: () => ({
		"24h": userIpUsage("24h"),
		"7d": userIpUsage("7d"),
	}),
	partialUserIpUsage: () => ({
		"24h": {
			...userIpUsage("24h"),
			partial: true,
			unreachable_nodes: [fixtureCatalog.identifier.nodeSecondary()],
		},
		"7d": {
			...userIpUsage("7d"),
			partial: true,
			unreachable_nodes: [fixtureCatalog.identifier.nodeSecondary()],
		},
	}),
	duplicateNameUserIpUsage: () => ({
		"24h": duplicateNameUserIpUsage("24h"),
		"7d": duplicateNameUserIpUsage("7d"),
	}),
	fiftyMeshPeers: () => fiftyMeshPeers(),
} as const;
