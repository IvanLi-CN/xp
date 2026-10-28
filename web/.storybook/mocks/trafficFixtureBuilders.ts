import type { AdminNode } from "../../src/api/adminNodes";
import type {
	AdminNodeTrafficResponse,
	AdminUserTrafficResponse,
	TrafficReport,
	TrafficSeriesPoint,
	TrafficWindow,
} from "../../src/api/adminTraffic";
import type { AdminUser } from "../../src/api/adminUsers";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import { catalogTimestampOffset } from "./apiMockFixtureBuilders";

const FIVE_MINUTES_MS = 5 * 60 * 1000;
const DAY_MS = 24 * 60 * 60 * 1000;
const BASELINE_MS = Date.parse(fixtureCatalog.timestamp.baseline());

type TrafficBytes = {
	uplink_bytes: number;
	downlink_bytes: number;
	total_bytes: number;
};

type TrafficFixtureProfile = {
	currentEven: TrafficBytes;
	currentOdd: TrafficBytes;
	referenceEven: TrafficBytes;
	referenceOdd: TrafficBytes;
	summary: TrafficBytes;
};

const PRIMARY_TRAFFIC_PROFILE: TrafficFixtureProfile = {
	currentEven: {
		uplink_bytes: fixtureCatalog.quota.oneGiB(),
		downlink_bytes: fixtureCatalog.quota.fourGiB(),
		total_bytes: fixtureCatalog.quota.fiveGiB(),
	},
	currentOdd: {
		uplink_bytes: fixtureCatalog.quota.fiveGiB(),
		downlink_bytes: fixtureCatalog.quota.tenGiB(),
		total_bytes: fixtureCatalog.quota.fifteenGiB(),
	},
	referenceEven: {
		uplink_bytes: fixtureCatalog.quota.oneGiB(),
		downlink_bytes: fixtureCatalog.quota.fourGiB(),
		total_bytes: fixtureCatalog.quota.fiveGiB(),
	},
	referenceOdd: {
		uplink_bytes: fixtureCatalog.quota.fourGiB(),
		downlink_bytes: fixtureCatalog.quota.fiveGiB(),
		total_bytes: fixtureCatalog.quota.tenGiB(),
	},
	summary: {
		uplink_bytes: fixtureCatalog.quota.fiveGiB(),
		downlink_bytes: fixtureCatalog.quota.tenGiB(),
		total_bytes: fixtureCatalog.quota.fifteenGiB(),
	},
};

const SECONDARY_TRAFFIC_PROFILE: TrafficFixtureProfile = {
	currentEven: {
		uplink_bytes: fixtureCatalog.quota.fourGiB(),
		downlink_bytes: fixtureCatalog.quota.elevenGiB(),
		total_bytes: fixtureCatalog.quota.fifteenGiB(),
	},
	currentOdd: {
		uplink_bytes: fixtureCatalog.quota.oneGiB(),
		downlink_bytes: fixtureCatalog.quota.fourGiB(),
		total_bytes: fixtureCatalog.quota.fiveGiB(),
	},
	referenceEven: {
		uplink_bytes: fixtureCatalog.quota.oneGiB(),
		downlink_bytes: fixtureCatalog.quota.fourGiB(),
		total_bytes: fixtureCatalog.quota.fiveGiB(),
	},
	referenceOdd: {
		uplink_bytes: fixtureCatalog.quota.fourGiB(),
		downlink_bytes: fixtureCatalog.quota.elevenGiB(),
		total_bytes: fixtureCatalog.quota.fifteenGiB(),
	},
	summary: {
		uplink_bytes: fixtureCatalog.quota.fourGiB(),
		downlink_bytes: fixtureCatalog.quota.elevenGiB(),
		total_bytes: fixtureCatalog.quota.fifteenGiB(),
	},
};

function trafficFixtureProfile(node: AdminNode): TrafficFixtureProfile {
	return node.node_id === fixtureCatalog.nodeId.fixture36()
		? SECONDARY_TRAFFIC_PROFILE
		: PRIMARY_TRAFFIC_PROFILE;
}

type WindowedNodeTraffic = Partial<
	Record<TrafficWindow, AdminNodeTrafficResponse>
>;
type WindowedUserTraffic = Partial<
	Record<TrafficWindow, AdminUserTrafficResponse>
>;

function buildTrafficPoint(
	window: TrafficWindow,
	index: number,
	isCurrentDay: boolean,
	windowStartMs: number,
	profile: TrafficFixtureProfile,
	isGap = false,
): TrafficSeriesPoint {
	const step = window === "24h" ? FIVE_MINUTES_MS : DAY_MS;
	const startMs = windowStartMs + index * step;
	const bytes = isGap
		? {
				uplink_bytes: fixtureCatalog.optional.none(),
				downlink_bytes: fixtureCatalog.optional.none(),
				total_bytes: fixtureCatalog.optional.none(),
			}
		: index % 2 === 0
			? profile.currentEven
			: profile.currentOdd;
	return {
		start_at: catalogTimestampOffset(startMs - BASELINE_MS),
		end_at: catalogTimestampOffset(startMs + step - BASELINE_MS),
		...bytes,
		complete: !isGap,
		is_current_day: isCurrentDay,
	};
}

function buildReferenceTrafficPoint(
	window: TrafficWindow,
	index: number,
	windowStartMs: number,
	profile: TrafficFixtureProfile,
): TrafficSeriesPoint {
	const step = window === "24h" ? FIVE_MINUTES_MS : DAY_MS;
	const count = window === "24h" ? 288 : 31;
	const startMs = windowStartMs + (index - count) * step;
	const bytes = index % 2 === 0 ? profile.referenceEven : profile.referenceOdd;
	return {
		start_at: catalogTimestampOffset(startMs - BASELINE_MS),
		end_at: catalogTimestampOffset(startMs + step - BASELINE_MS),
		...bytes,
		complete: true,
		is_current_day: false,
	};
}

function trafficWindowStart(
	window: TrafficWindow,
	trafficNowMs: number | undefined,
): number {
	if (trafficNowMs === undefined) {
		return Date.parse(fixtureCatalog.timestamp.baseline());
	}
	if (window === "24h") {
		const end = Math.floor(trafficNowMs / FIVE_MINUTES_MS) * FIVE_MINUTES_MS;
		return end - DAY_MS;
	}
	const current = new Date(trafficNowMs);
	const currentDay = Date.UTC(
		current.getUTCFullYear(),
		current.getUTCMonth(),
		current.getUTCDate(),
	);
	return currentDay - 30 * DAY_MS;
}

function buildTrafficReport(
	window: TrafficWindow,
	gap = false,
	trafficNowMs?: number,
	profile: TrafficFixtureProfile = PRIMARY_TRAFFIC_PROFILE,
): TrafficReport {
	const count = window === "24h" ? 288 : 31;
	const step = window === "24h" ? FIVE_MINUTES_MS : DAY_MS;
	const windowStartMs = trafficWindowStart(window, trafficNowMs);
	const windowEndMs = windowStartMs + count * step;
	const current = Array.from({ length: count }, (_, index) =>
		buildTrafficPoint(
			window,
			index,
			window === "31d" && index === count - 1,
			windowStartMs,
			profile,
			gap && index > 112 && index < 124,
		),
	);
	const reference = current.map((_, index) =>
		buildReferenceTrafficPoint(window, index, windowStartMs, profile),
	);
	return {
		window,
		window_start_at: catalogTimestampOffset(windowStartMs - BASELINE_MS),
		window_end_at: catalogTimestampOffset(windowEndMs - BASELINE_MS),
		timezone: "UTC",
		summary: {
			mode: "cycle",
			cycle_start_at: fixtureCatalog.timestamp.baseline(),
			cycle_end_at: catalogTimestampOffset(windowEndMs - BASELINE_MS),
			...profile.summary,
			complete: !gap,
			tracking_since: catalogTimestampOffset(windowStartMs - BASELINE_MS),
		},
		current,
		reference,
		partial: false,
		last_sample_at: catalogTimestampOffset(windowEndMs - BASELINE_MS),
		warnings: [],
	};
}

export function buildDefaultNodeTraffic(
	node: AdminNode,
	trafficNowMs?: number,
): WindowedNodeTraffic {
	const profile = trafficFixtureProfile(node);
	return {
		"24h": {
			node,
			traffic: buildTrafficReport("24h", false, trafficNowMs, profile),
		},
		"31d": {
			node,
			traffic: buildTrafficReport("31d", false, trafficNowMs, profile),
		},
	};
}

export function buildDefaultUserTraffic(
	user: AdminUser,
	nodes: AdminNode[],
	trafficNowMs?: number,
): WindowedUserTraffic {
	const nodeOptions = nodes.map((node, index) => {
		const option = {
			node_id: fixtureCatalog.nodeId.fixture36(),
			node_name: fixtureCatalog.nodeName.fixture37(),
		};
		if (node.node_id === fixtureCatalog.identifier.nodePrimary()) {
			option.node_id = fixtureCatalog.identifier.nodePrimary();
			option.node_name = fixtureCatalog.identifier.nodeNameSecondary();
		} else if (node.node_id === fixtureCatalog.identifier.nodeSecondary()) {
			option.node_id = fixtureCatalog.identifier.nodeSecondary();
			option.node_name = fixtureCatalog.identifier.nodeNamePrimary();
		} else if (index === 0) {
			option.node_id = fixtureCatalog.nodeId.fixture32();
			option.node_name = fixtureCatalog.nodeName.fixture33();
		}
		return option;
	});
	return {
		"24h": {
			user: { user_id: user.user_id, display_name: user.display_name },
			traffic: buildTrafficReport("24h", false, trafficNowMs),
			nodes: nodeOptions,
			partial: false,
			unreachable_nodes: [],
		},
		"31d": {
			user: { user_id: user.user_id, display_name: user.display_name },
			traffic: buildTrafficReport("31d", false, trafficNowMs),
			nodes: nodeOptions,
			partial: false,
			unreachable_nodes: [],
		},
	};
}
