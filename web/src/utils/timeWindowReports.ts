import type {
	AdminIpUsageWindow,
	AdminNodeIpUsageResponse,
	AdminUserIpUsageNodeGroup,
	AdminUserIpUsageResponse,
} from "../api/adminIpUsage";
import type {
	AdminNodeTcpConnectionsResponse,
	AdminTcpConnectionUsageWindow,
} from "../api/adminTcpConnections";
import type {
	AdminNodeTrafficResponse,
	AdminUserTrafficResponse,
	TrafficReport,
	TrafficSeriesPoint,
	TrafficWindow,
} from "../api/adminTraffic";

const MINUTE_MS = 60_000;
const FIVE_MINUTES_MS = 5 * MINUTE_MS;
const DAY_MS = 24 * 60 * MINUTE_MS;

type WindowRange = {
	end: number;
	start: number;
};

function floorTo(value: number, step: number): number {
	return Math.floor(value / step) * step;
}

function minuteRange(
	window: AdminIpUsageWindow | AdminTcpConnectionUsageWindow,
	now: number,
): WindowRange {
	const end = floorTo(now, MINUTE_MS);
	const minutes = window === "24h" ? 24 * 60 : 7 * 24 * 60;
	return { end, start: end - (minutes - 1) * MINUTE_MS };
}

function trafficRange(window: TrafficWindow, now: number): WindowRange {
	if (window === "24h") {
		const end = floorTo(now, FIVE_MINUTES_MS);
		return { end, start: end - DAY_MS };
	}
	const current = new Date(now);
	const currentDay = Date.UTC(
		current.getUTCFullYear(),
		current.getUTCMonth(),
		current.getUTCDate(),
	);
	return { end: currentDay + DAY_MS, start: currentDay - 30 * DAY_MS };
}

function iso(value: number): string {
	return new Date(value).toISOString();
}

function inRange(value: string, range: WindowRange): boolean {
	const timestamp = new Date(value).getTime();
	return (
		Number.isFinite(timestamp) &&
		timestamp >= range.start &&
		timestamp <= range.end
	);
}

function alignIpGroup<
	T extends AdminUserIpUsageNodeGroup | AdminNodeIpUsageResponse,
>(report: T, window: AdminIpUsageWindow, now: number): T {
	const range = minuteRange(window, now);
	const rangeChanged =
		new Date(report.window_start).getTime() !== range.start ||
		new Date(report.window_end).getTime() !== range.end;
	const timeline = report.timeline.flatMap((lane) => {
		const segments = lane.segments.flatMap((segment) => {
			const start = Math.max(
				new Date(segment.start_minute).getTime(),
				range.start,
			);
			const end = Math.min(new Date(segment.end_minute).getTime(), range.end);
			return Number.isFinite(start) && Number.isFinite(end) && start <= end
				? [{ start_minute: iso(start), end_minute: iso(end) }]
				: [];
		});
		if (segments.length === 0) return [];
		const minutes = segments.reduce(
			(total, segment) =>
				total +
				Math.floor(
					(new Date(segment.end_minute).getTime() -
						new Date(segment.start_minute).getTime()) /
						MINUTE_MS,
				) +
				1,
			0,
		);
		return [{ ...lane, minutes, segments }];
	});

	return {
		...report,
		window_start: iso(range.start),
		window_end: iso(range.end),
		unique_ip_series: report.unique_ip_series.filter((point) =>
			inRange(point.minute, range),
		),
		timeline,
		ips: rangeChanged ? [] : report.ips,
	};
}

function emptyIpGroup<
	T extends AdminUserIpUsageNodeGroup | AdminNodeIpUsageResponse,
>(report: T, window: AdminIpUsageWindow, now: number): T {
	return alignIpGroup(
		{
			...report,
			warnings: [],
			unique_ip_series: [],
			timeline: [],
			ips: [],
		},
		window,
		now,
	);
}

export function alignNodeIpUsageResponse(
	report: AdminNodeIpUsageResponse,
	window: AdminIpUsageWindow,
	now: number,
): AdminNodeIpUsageResponse {
	return { ...alignIpGroup(report, window, now), window };
}

export function emptyNodeIpUsageResponse(
	report: AdminNodeIpUsageResponse,
	window: AdminIpUsageWindow,
	now: number,
): AdminNodeIpUsageResponse {
	return { ...emptyIpGroup(report, window, now), window };
}

export function alignUserIpUsageResponse(
	report: AdminUserIpUsageResponse,
	window: AdminIpUsageWindow,
	now: number,
): AdminUserIpUsageResponse {
	return {
		...report,
		window,
		groups: report.groups.map((group) => alignIpGroup(group, window, now)),
	};
}

export function emptyUserIpUsageResponse(
	report: AdminUserIpUsageResponse,
	window: AdminIpUsageWindow,
	now: number,
): AdminUserIpUsageResponse {
	return {
		...report,
		window,
		partial: false,
		unreachable_nodes: [],
		warnings: [],
		groups: report.groups.map((group) => emptyIpGroup(group, window, now)),
	};
}

export function alignNodeTcpConnectionsResponse(
	report: AdminNodeTcpConnectionsResponse,
	window: AdminTcpConnectionUsageWindow,
	now: number,
): AdminNodeTcpConnectionsResponse {
	const range = minuteRange(window, now);
	return {
		...report,
		window,
		window_start: iso(range.start),
		window_end: iso(range.end),
		per_endpoint_series: report.per_endpoint_series.map((series) => ({
			...series,
			series: series.series.filter((point) => inRange(point.minute, range)),
		})),
	};
}

export function emptyNodeTcpConnectionsResponse(
	report: AdminNodeTcpConnectionsResponse,
	window: AdminTcpConnectionUsageWindow,
	now: number,
): AdminNodeTcpConnectionsResponse {
	return alignNodeTcpConnectionsResponse(
		{
			...report,
			warnings: [],
			per_endpoint_series: report.per_endpoint_series.map((series) => ({
				...series,
				series: [],
			})),
		},
		window,
		now,
	);
}

function blankTrafficPoint(
	start: number,
	step: number,
	currentDay: boolean,
): TrafficSeriesPoint {
	return {
		start_at: iso(start),
		end_at: iso(start + step),
		uplink_bytes: null,
		downlink_bytes: null,
		total_bytes: null,
		complete: false,
		is_current_day: currentDay,
	};
}

function alignTrafficReport(
	report: TrafficReport,
	window: TrafficWindow,
	now: number,
	empty: boolean,
): TrafficReport {
	const range = trafficRange(window, now);
	const count = window === "24h" ? 288 : 31;
	const step = window === "24h" ? FIVE_MINUTES_MS : DAY_MS;
	const source = empty ? [] : [...report.current, ...(report.reference ?? [])];
	const byStart = new Map(
		source.flatMap((point) => {
			const start = new Date(point.start_at).getTime();
			return Number.isFinite(start) ? [[start, point] as const] : [];
		}),
	);
	const pointAt = (start: number, currentDay: boolean) => {
		const point = byStart.get(start);
		return point
			? { ...point, is_current_day: currentDay }
			: blankTrafficPoint(start, step, currentDay);
	};
	const current = Array.from({ length: count }, (_, index) => {
		const start = range.start + index * step;
		return pointAt(start, window === "31d" && index === count - 1);
	});
	const referenceStart = range.start - count * step;
	const reference = Array.from({ length: count }, (_, index) =>
		pointAt(referenceStart + index * step, false),
	);

	return {
		...report,
		window,
		window_start_at: iso(range.start),
		window_end_at: iso(range.end),
		current,
		reference,
		summary: empty
			? {
					...report.summary,
					uplink_bytes: 0,
					downlink_bytes: 0,
					total_bytes: 0,
					complete: false,
				}
			: report.summary,
		partial:
			empty || report.partial || current.some((point) => !point.complete),
		last_sample_at: empty ? null : report.last_sample_at,
		warnings: empty ? [] : report.warnings,
	};
}

export function alignNodeTrafficResponse(
	response: AdminNodeTrafficResponse,
	window: TrafficWindow,
	now: number,
): AdminNodeTrafficResponse {
	return {
		...response,
		traffic: alignTrafficReport(response.traffic, window, now, false),
	};
}

export function emptyNodeTrafficResponse(
	response: AdminNodeTrafficResponse,
	window: TrafficWindow,
	now: number,
): AdminNodeTrafficResponse {
	return {
		...response,
		traffic: alignTrafficReport(response.traffic, window, now, true),
	};
}

export function alignUserTrafficResponse(
	response: AdminUserTrafficResponse,
	window: TrafficWindow,
	now: number,
): AdminUserTrafficResponse {
	return {
		...response,
		traffic: alignTrafficReport(response.traffic, window, now, false),
	};
}

export function emptyUserTrafficResponse(
	response: AdminUserTrafficResponse,
	window: TrafficWindow,
	now: number,
): AdminUserTrafficResponse {
	return {
		...response,
		traffic: alignTrafficReport(response.traffic, window, now, true),
	};
}
