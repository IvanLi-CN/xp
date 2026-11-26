import type {
	AdminIpUsageWindow,
	AdminNodeIpUsageResponse,
	AdminUserIpUsageResponse,
} from "../api/adminIpUsage";
import type { AdminMeshPeer } from "../api/adminMesh";
import type { AdminNode } from "../api/adminNodes";
import type {
	AdminNodeTcpConnectionsResponse,
	AdminTcpConnectionHistoryWindow,
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

function totalMinutesFor(window: AdminTcpConnectionHistoryWindow): number {
	switch (window) {
		case "24h":
			return 24 * 60;
		case "7d":
			return 7 * 24 * 60;
		case "30d":
			return 30 * 24 * 60;
		case "90d":
			return 90 * 24 * 60;
	}
}

function buildSeries(
	window: AdminTcpConnectionHistoryWindow,
	windowStart: string,
	seeds: EndpointSeed[],
) {
	const totalMinutes = totalMinutesFor(window);
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

function fullSiteFields(
	window: AdminTcpConnectionHistoryWindow,
	windowStart: string,
	perEndpointSeries: AdminNodeTcpConnectionsResponse["per_endpoint_series"],
): Pick<
	AdminNodeTcpConnectionsResponse,
	| "quality"
	| "reasons"
	| "truncated"
	| "truncated_buckets"
	| "partial_buckets"
	| "deleted_subjects"
	| "subjects_truncated"
	| "subjects_omitted"
	| "subjects_next_cursor"
	| "resolution"
	| "watermark"
	| "gaps"
	| "total"
	| "classes"
	| "subjects"
> {
	const resolution = window === "24h" ? "1m" : window === "7d" ? "5m" : "daily";
	const stepMinutes = resolution === "1m" ? 1 : resolution === "5m" ? 5 : 60;
	const stepMs = stepMinutes * ONE_MINUTE_MS;
	const windowMinutes = totalMinutesFor(window);
	const bucketCount = Math.max(2, Math.floor(windowMinutes / stepMinutes));
	const startMs = new Date(windowStart).getTime();
	// Broad daily rhythm: four wide waves across the window, not a per-minute comb.
	const wave = (index: number) => {
		const phase = (index / bucketCount) * Math.PI * 4;
		return Math.max(
			0,
			Math.round(8 + 5 * Math.sin(phase) + 2 * Math.sin(phase / 3)),
		);
	};
	const bucketPoint = (bucket: number, value: number) => ({
		bucket_start: new Date(bucket).toISOString(),
		min: value,
		avg: value,
		max: value,
		samples_expected: stepMinutes,
		samples_actual: stepMinutes,
		truncated: false,
		partial: false,
	});
	const totalPoints = Array.from({ length: bucketCount }, (_, index) =>
		bucketPoint(startMs + index * stepMs, wave(index)),
	);
	const scale = (factor: number) =>
		totalPoints.map((point) => ({
			...point,
			min: Math.round(point.min * factor),
			avg: Math.round(point.avg * factor),
			max: Math.round(point.max * factor),
		}));
	return {
		quality: "complete",
		reasons: [],
		truncated: false,
		truncated_buckets: [],
		partial_buckets: [],
		deleted_subjects: [],
		subjects_truncated: false,
		subjects_omitted: false,
		subjects_next_cursor: null,
		resolution,
		watermark: totalPoints.at(-1)?.bucket_start ?? null,
		gaps: [],
		total: { approximate: false, points: totalPoints },
		classes: [
			{
				class: "inbound",
				approximate: resolution === "daily",
				points: scale(0.55),
			},
			{
				class: "control_plane",
				approximate: resolution === "daily",
				points: scale(0.25),
			},
			{
				class: "outbound_runtime",
				approximate: resolution === "daily",
				points: scale(0.15),
			},
			{
				class: "outbound_other",
				approximate: resolution === "daily",
				points: scale(0.05),
			},
		],
		subjects: perEndpointSeries.map((series) => ({
			class: "inbound",
			subject: series.endpoint_id,
			approximate: false,
			points: series.series
				.filter((item) => item.count > 0)
				.map((item) =>
					bucketPoint(new Date(item.minute).getTime(), item.count),
				),
		})),
	};
}

function report(
	window: AdminTcpConnectionHistoryWindow,
	windowStart: string,
	seeds: EndpointSeed[],
): AdminNodeTcpConnectionsResponse {
	const totalMinutes = totalMinutesFor(window);
	// Long-term windows are served from repository daily payloads; the legacy per-endpoint
	// series is intentionally empty there, matching the backend contract.
	const perEndpointSeries =
		window === "24h" || window === "7d"
			? buildSeries(window, windowStart, seeds)
			: [];
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
		per_endpoint_series: perEndpointSeries,
		...fullSiteFields(window, windowStart, perEndpointSeries),
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
		endpointTag: fixtureCatalog.identifier.endpointTagTertiary(),
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
		endpointTag: fixtureCatalog.identifier.endpointTagTertiary(),
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
					node_name: fixtureCatalog.label.fixtureDuplicate(),
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
					node_name: fixtureCatalog.label.fixtureDuplicate(),
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
		node_name: fixtureCatalog.nodeName.fixture291(),
		api_base_url: fixtureCatalog.url.primaryApi(),
		mesh_url: fixtureCatalog.url.primaryApi(),
		mesh_capability: "enabled",
		mesh_reason: "mesh_available",
		current_path: "mesh",
		quality: "good",
		stale: false,
		breaker: "closed",
		direct_connectivity: "verified",
		public_fallback_health: "available",
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
		"30d": report("30d", fixtureCatalog.timestamp.baseline(), seeds7d),
		"90d": report("90d", fixtureCatalog.timestamp.baseline(), seeds7d),
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
