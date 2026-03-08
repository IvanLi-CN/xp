import type {
	AdminIpUsageListEntry,
	AdminIpUsageSeriesPoint,
	AdminIpUsageTimelineLane,
	AdminIpUsageWarning,
	AdminIpUsageWindow,
	AdminNodeIpUsageResponse,
	AdminUserIpUsageNodeGroup,
	AdminUserIpUsageResponse,
} from "../api/adminIpUsage";
import type { AdminNode } from "../api/adminNodes";
import type { AdminUser } from "../api/adminUsers";

type SharedIpUsageReport = Pick<
	AdminNodeIpUsageResponse,
	| "window_start"
	| "window_end"
	| "warnings"
	| "unique_ip_series"
	| "timeline"
	| "ips"
>;

type SegmentRange = readonly [startMinute: number, endMinute: number];

type LaneSeed = {
	endpoint_id: string;
	endpoint_tag: string;
	ip: string;
	region: string;
	operator: string;
	segments: SegmentRange[];
};

export type WindowedNodeIpUsageStories = Record<
	AdminIpUsageWindow,
	AdminNodeIpUsageResponse
>;
export type WindowedUserIpUsageStories = Record<
	AdminIpUsageWindow,
	AdminUserIpUsageResponse
>;

const ONE_MINUTE_MS = 60_000;
const DAY_MINUTES = 24 * 60;

function isoMinuteFromOffset(
	startMinuteIso: string,
	offsetMinutes: number,
): string {
	const baseMs = new Date(startMinuteIso).getTime();
	return new Date(baseMs + offsetMinutes * ONE_MINUTE_MS).toISOString();
}

function buildReportFromLanes(
	window: AdminIpUsageWindow,
	windowStart: string,
	lanes: LaneSeed[],
	warnings: AdminIpUsageWarning[] = [],
): SharedIpUsageReport {
	const totalMinutes = window === "24h" ? 24 * 60 : 7 * 24 * 60;
	const activeIpsByMinute = new Map<number, Set<string>>();
	const ipMinutes = new Map<string, Set<number>>();
	const ipEndpointTags = new Map<string, Set<string>>();
	const ipRegions = new Map<string, string>();
	const ipOperators = new Map<string, string>();
	const ipLastSeen = new Map<string, number>();

	const timeline: AdminIpUsageTimelineLane[] = lanes.map((lane) => {
		let minutes = 0;
		for (const [startMinute, endMinute] of lane.segments) {
			minutes += endMinute - startMinute + 1;
			for (let minute = startMinute; minute <= endMinute; minute += 1) {
				const activeIps = activeIpsByMinute.get(minute) ?? new Set<string>();
				activeIps.add(lane.ip);
				activeIpsByMinute.set(minute, activeIps);

				const minutesForIp = ipMinutes.get(lane.ip) ?? new Set<number>();
				minutesForIp.add(minute);
				ipMinutes.set(lane.ip, minutesForIp);

				const endpointTags = ipEndpointTags.get(lane.ip) ?? new Set<string>();
				endpointTags.add(lane.endpoint_tag);
				ipEndpointTags.set(lane.ip, endpointTags);
				ipRegions.set(lane.ip, lane.region);
				ipOperators.set(lane.ip, lane.operator);
				ipLastSeen.set(lane.ip, Math.max(ipLastSeen.get(lane.ip) ?? 0, minute));
			}
		}

		return {
			lane_key: `${lane.endpoint_tag}::${lane.ip}`,
			endpoint_id: lane.endpoint_id,
			endpoint_tag: lane.endpoint_tag,
			ip: lane.ip,
			minutes,
			segments: lane.segments.map(([startMinute, endMinute]) => ({
				start_minute: isoMinuteFromOffset(windowStart, startMinute),
				end_minute: isoMinuteFromOffset(windowStart, endMinute),
			})),
		};
	});

	const unique_ip_series: AdminIpUsageSeriesPoint[] = Array.from(
		{ length: totalMinutes },
		(_, minute) => ({
			minute: isoMinuteFromOffset(windowStart, minute),
			count: activeIpsByMinute.get(minute)?.size ?? 0,
		}),
	);

	const ips: AdminIpUsageListEntry[] = Array.from(ipMinutes.entries())
		.map(([ip, minutes]) => ({
			ip,
			minutes: minutes.size,
			endpoint_tags: Array.from(ipEndpointTags.get(ip) ?? []).sort(),
			region: ipRegions.get(ip) ?? "",
			operator: ipOperators.get(ip) ?? "",
			last_seen_at: isoMinuteFromOffset(windowStart, ipLastSeen.get(ip) ?? 0),
		}))
		.sort((left, right) => {
			if (right.minutes !== left.minutes) return right.minutes - left.minutes;
			return left.ip.localeCompare(right.ip);
		});

	return {
		window_start: windowStart,
		window_end: isoMinuteFromOffset(windowStart, totalMinutes - 1),
		warnings,
		unique_ip_series,
		timeline: timeline.sort((left, right) => {
			if (right.minutes !== left.minutes) return right.minutes - left.minutes;
			return left.endpoint_tag.localeCompare(right.endpoint_tag);
		}),
		ips,
	};
}

function repeatDailySegments(
	dailySegments: SegmentRange[],
	days: number,
	jitterByDay = 0,
): SegmentRange[] {
	const ranges: SegmentRange[] = [];
	for (let day = 0; day < days; day += 1) {
		const shift = day * jitterByDay;
		for (const [startMinute, endMinute] of dailySegments) {
			ranges.push([
				day * DAY_MINUTES + startMinute + shift,
				day * DAY_MINUTES + endMinute + shift,
			]);
		}
	}
	return ranges;
}

function buildNodeLanes24h(node: AdminNode): LaneSeed[] {
	const prefix = node.node_name || node.node_id;
	return [
		{
			endpoint_id: `${node.node_id}-endpoint-a`,
			endpoint_tag: `${prefix}-edge-a`,
			ip: "203.0.113.7",
			region: "Japan / Tokyo",
			operator: "ExampleNet",
			segments: [
				[5, 95],
				[130, 210],
				[355, 500],
				[780, 915],
				[1100, 1230],
			],
		},
		{
			endpoint_id: `${node.node_id}-endpoint-a`,
			endpoint_tag: `${prefix}-edge-a`,
			ip: "203.0.113.11",
			region: "Japan / Saitama",
			operator: "ExampleNet",
			segments: [
				[40, 150],
				[540, 620],
				[960, 1010],
				[1260, 1380],
			],
		},
		{
			endpoint_id: `${node.node_id}-endpoint-b`,
			endpoint_tag: `${prefix}-edge-b`,
			ip: "198.51.100.22",
			region: "South Korea / Seoul",
			operator: "Hanabit",
			segments: [
				[180, 260],
				[400, 520],
				[840, 900],
			],
		},
		{
			endpoint_id: `${node.node_id}-endpoint-b`,
			endpoint_tag: `${prefix}-edge-b`,
			ip: "203.0.113.7",
			region: "Japan / Tokyo",
			operator: "ExampleNet",
			segments: [
				[220, 240],
				[930, 945],
			],
		},
		{
			endpoint_id: `${node.node_id}-endpoint-c`,
			endpoint_tag: `${prefix}-edge-c`,
			ip: "198.51.100.24",
			region: "Singapore",
			operator: "LionLink",
			segments: [
				[250, 330],
				[680, 760],
				[1180, 1280],
			],
		},
		{
			endpoint_id: `${node.node_id}-endpoint-c`,
			endpoint_tag: `${prefix}-edge-c`,
			ip: "192.0.2.33",
			region: "Hong Kong",
			operator: "Victoria Mobile",
			segments: [
				[0, 40],
				[300, 380],
				[720, 810],
				[1020, 1080],
			],
		},
		{
			endpoint_id: `${node.node_id}-endpoint-d`,
			endpoint_tag: `${prefix}-edge-d`,
			ip: "192.0.2.44",
			region: "United States / California",
			operator: "West Carrier",
			segments: [
				[90, 120],
				[600, 690],
				[900, 980],
				[1320, 1439],
			],
		},
		{
			endpoint_id: `${node.node_id}-endpoint-d`,
			endpoint_tag: `${prefix}-edge-d`,
			ip: "203.0.113.55",
			region: "Japan / Osaka",
			operator: "CarrierNet",
			segments: [
				[150, 170],
				[430, 450],
				[870, 920],
				[1110, 1190],
			],
		},
		{
			endpoint_id: `${node.node_id}-endpoint-e`,
			endpoint_tag: `${prefix}-edge-e`,
			ip: "198.51.100.88",
			region: "Germany / Frankfurt",
			operator: "EuroFiber",
			segments: [
				[510, 590],
				[1000, 1160],
			],
		},
	];
}

function buildNodeLanes7d(node: AdminNode): LaneSeed[] {
	const prefix = node.node_name || node.node_id;
	return [
		{
			endpoint_id: `${node.node_id}-endpoint-a`,
			endpoint_tag: `${prefix}-edge-a`,
			ip: "203.0.113.7",
			region: "Japan / Tokyo",
			operator: "ExampleNet",
			segments: repeatDailySegments(
				[
					[10, 120],
					[320, 430],
					[700, 860],
					[1080, 1210],
				],
				7,
				3,
			),
		},
		{
			endpoint_id: `${node.node_id}-endpoint-a`,
			endpoint_tag: `${prefix}-edge-a`,
			ip: "203.0.113.11",
			region: "Japan / Saitama",
			operator: "ExampleNet",
			segments: repeatDailySegments(
				[
					[180, 260],
					[540, 630],
					[1260, 1380],
				],
				7,
				5,
			),
		},
		{
			endpoint_id: `${node.node_id}-endpoint-b`,
			endpoint_tag: `${prefix}-edge-b`,
			ip: "198.51.100.22",
			region: "South Korea / Seoul",
			operator: "Hanabit",
			segments: repeatDailySegments(
				[
					[90, 160],
					[450, 570],
					[840, 930],
				],
				7,
				2,
			),
		},
		{
			endpoint_id: `${node.node_id}-endpoint-b`,
			endpoint_tag: `${prefix}-edge-b`,
			ip: "203.0.113.7",
			region: "Japan / Tokyo",
			operator: "ExampleNet",
			segments: repeatDailySegments([[1180, 1225]], 7, 1),
		},
		{
			endpoint_id: `${node.node_id}-endpoint-c`,
			endpoint_tag: `${prefix}-edge-c`,
			ip: "198.51.100.24",
			region: "Singapore",
			operator: "LionLink",
			segments: repeatDailySegments(
				[
					[210, 300],
					[610, 730],
					[980, 1040],
				],
				7,
				4,
			),
		},
		{
			endpoint_id: `${node.node_id}-endpoint-c`,
			endpoint_tag: `${prefix}-edge-c`,
			ip: "192.0.2.33",
			region: "Hong Kong",
			operator: "Victoria Mobile",
			segments: repeatDailySegments(
				[
					[0, 45],
					[360, 420],
					[900, 1020],
				],
				7,
				0,
			),
		},
		{
			endpoint_id: `${node.node_id}-endpoint-d`,
			endpoint_tag: `${prefix}-edge-d`,
			ip: "192.0.2.44",
			region: "United States / California",
			operator: "West Carrier",
			segments: repeatDailySegments(
				[
					[60, 90],
					[780, 920],
					[1320, 1439],
				],
				7,
				6,
			),
		},
		{
			endpoint_id: `${node.node_id}-endpoint-d`,
			endpoint_tag: `${prefix}-edge-d`,
			ip: "203.0.113.55",
			region: "Japan / Osaka",
			operator: "CarrierNet",
			segments: repeatDailySegments(
				[
					[240, 320],
					[660, 720],
					[1110, 1180],
				],
				7,
				2,
			),
		},
		{
			endpoint_id: `${node.node_id}-endpoint-e`,
			endpoint_tag: `${prefix}-edge-e`,
			ip: "198.51.100.88",
			region: "Germany / Frankfurt",
			operator: "EuroFiber",
			segments: repeatDailySegments(
				[
					[500, 620],
					[1020, 1130],
				],
				7,
				5,
			),
		},
	];
}

function buildUserGroup(
	node: AdminNode,
	lanes: LaneSeed[],
	window: AdminIpUsageWindow,
): AdminUserIpUsageNodeGroup {
	const start =
		window === "24h" ? "2026-03-07T00:00:00Z" : "2026-03-01T00:00:00Z";
	return {
		node,
		...buildReportFromLanes(window, start, lanes),
	};
}

export function buildDenseNodeIpUsageStories(
	node: AdminNode,
): WindowedNodeIpUsageStories {
	return {
		"24h": {
			node,
			window: "24h",
			...buildReportFromLanes(
				"24h",
				"2026-03-07T00:00:00Z",
				buildNodeLanes24h(node),
				[
					{
						code: "geo_db_missing",
						message:
							"GeoLite2 City/ASN DB is missing; region and operator fields will be empty.",
					},
				],
			),
		},
		"7d": {
			node,
			window: "7d",
			...buildReportFromLanes(
				"7d",
				"2026-03-01T00:00:00Z",
				buildNodeLanes7d(node),
				[
					{
						code: "geo_db_missing",
						message:
							"GeoLite2 City/ASN DB is missing; region and operator fields will be empty.",
					},
				],
			),
		},
	};
}

function buildUserLanes(
	node: AdminNode,
	window: AdminIpUsageWindow,
	variant: number,
): LaneSeed[] {
	const prefix = node.node_name || node.node_id;
	const isTokyo = variant === 0;
	const lanes24h: LaneSeed[] = isTokyo
		? [
				{
					endpoint_id: `${node.node_id}-endpoint-1`,
					endpoint_tag: `${prefix}-edge-1`,
					ip: "203.0.113.7",
					region: "Japan / Tokyo",
					operator: "ExampleNet",
					segments: [
						[30, 200],
						[420, 540],
						[860, 990],
						[1180, 1290],
					],
				},
				{
					endpoint_id: `${node.node_id}-endpoint-1`,
					endpoint_tag: `${prefix}-edge-1`,
					ip: "203.0.113.19",
					region: "Japan / Yokohama",
					operator: "MetroLink",
					segments: [
						[240, 340],
						[620, 710],
						[1080, 1160],
					],
				},
				{
					endpoint_id: `${node.node_id}-endpoint-2`,
					endpoint_tag: `${prefix}-edge-2`,
					ip: "198.51.100.44",
					region: "South Korea / Seoul",
					operator: "Hanabit",
					segments: [
						[110, 170],
						[520, 660],
						[900, 980],
					],
				},
				{
					endpoint_id: `${node.node_id}-endpoint-2`,
					endpoint_tag: `${prefix}-edge-2`,
					ip: "198.51.100.57",
					region: "Singapore",
					operator: "LionLink",
					segments: [
						[0, 60],
						[360, 430],
						[760, 840],
						[1320, 1439],
					],
				},
			]
		: [
				{
					endpoint_id: `${node.node_id}-endpoint-1`,
					endpoint_tag: `${prefix}-edge-1`,
					ip: "203.0.113.8",
					region: "Japan / Osaka",
					operator: "CarrierNet",
					segments: [
						[20, 180],
						[500, 640],
						[940, 1090],
					],
				},
				{
					endpoint_id: `${node.node_id}-endpoint-1`,
					endpoint_tag: `${prefix}-edge-1`,
					ip: "203.0.113.41",
					region: "Japan / Kobe",
					operator: "CarrierNet",
					segments: [
						[260, 330],
						[700, 760],
						[1200, 1280],
					],
				},
				{
					endpoint_id: `${node.node_id}-endpoint-2`,
					endpoint_tag: `${prefix}-edge-2`,
					ip: "192.0.2.81",
					region: "Hong Kong",
					operator: "Victoria Mobile",
					segments: [
						[80, 120],
						[380, 470],
						[800, 920],
						[1100, 1180],
					],
				},
				{
					endpoint_id: `${node.node_id}-endpoint-2`,
					endpoint_tag: `${prefix}-edge-2`,
					ip: "198.51.100.99",
					region: "United States / California",
					operator: "West Carrier",
					segments: [
						[140, 210],
						[560, 620],
						[980, 1040],
						[1360, 1439],
					],
				},
			];

	if (window === "24h") return lanes24h;

	return lanes24h.map((lane, index) => ({
		...lane,
		segments: repeatDailySegments(lane.segments, 7, (index % 3) + 1),
	}));
}

export function buildDenseUserIpUsageStories(
	user: Pick<AdminUser, "user_id" | "display_name">,
	nodes: AdminNode[],
): WindowedUserIpUsageStories {
	const groups24h = nodes
		.slice(0, 2)
		.map((node, index) =>
			buildUserGroup(node, buildUserLanes(node, "24h", index), "24h"),
		);
	const groups7d = nodes
		.slice(0, 2)
		.map((node, index) =>
			buildUserGroup(node, buildUserLanes(node, "7d", index), "7d"),
		);
	if (groups24h[1]) {
		groups24h[1].warnings = [
			{
				code: "geo_db_missing",
				message:
					"GeoLite2 City/ASN DB is missing; region and operator fields will be empty.",
			},
		];
	}
	if (groups7d[1]) {
		groups7d[1].warnings = [
			{
				code: "geo_db_missing",
				message:
					"GeoLite2 City/ASN DB is missing; region and operator fields will be empty.",
			},
		];
	}

	return {
		"24h": {
			user: {
				user_id: user.user_id,
				display_name: user.display_name,
			},
			window: "24h",
			partial: false,
			unreachable_nodes: [],
			warnings: [],
			groups: groups24h,
		},
		"7d": {
			user: {
				user_id: user.user_id,
				display_name: user.display_name,
			},
			window: "7d",
			partial: false,
			unreachable_nodes: [],
			warnings: [],
			groups: groups7d,
		},
	};
}
