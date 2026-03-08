import type { CustomSeriesRenderItem, EChartsOption } from "echarts";
import ReactEChartsCore from "echarts-for-react/lib/core";
import type { EChartsReactProps } from "echarts-for-react/lib/types";
import { CustomChart, LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { SVGRenderer } from "echarts/renderers";
import { useCallback, useMemo, useRef, useState } from "react";

import type {
	AdminIpUsageListEntry,
	AdminIpUsageTimelineLane,
	AdminIpUsageWarning,
	AdminIpUsageWindow,
	AdminNodeIpUsageResponse,
	AdminUserIpUsageNodeGroup,
} from "../api/adminIpUsage";

const SVG_RENDERER = { renderer: "svg" } as const;
const AREA_CHART_HEIGHT = 224;
const MINUTE_MS = 60_000;

const CHART_GRID_COLOR = "rgba(148, 163, 184, 0.14)";
const CHART_AXIS_COLOR = "rgba(148, 163, 184, 0.55)";
const CHART_LINE_FALLBACK = "rgb(34, 211, 238)";
const CHART_AREA_START_FALLBACK = "rgba(34, 211, 238, 0.32)";
const CHART_AREA_END_FALLBACK = "rgba(34, 211, 238, 0.05)";
const HIGHLIGHT_IP_BAND_FALLBACK = "rgba(103, 232, 249, 0.10)";
const HIGHLIGHT_TIME_BAND_FALLBACK = "rgba(251, 191, 36, 0.14)";
const HIGHLIGHT_TIME_POINT_FALLBACK = "rgb(251, 191, 36)";
const LANE_FILL_FALLBACK = "rgba(56, 189, 248, 0.26)";
const LANE_BORDER_FALLBACK = "rgba(103, 232, 249, 0.92)";
const LANE_ACTIVE_FILL_FALLBACK = "rgba(34, 211, 238, 0.44)";
const LANE_ACTIVE_BORDER_FALLBACK = "rgba(165, 243, 252, 1)";
const LANE_MUTED_FILL_FALLBACK = "rgba(71, 85, 105, 0.16)";
const LANE_MUTED_BORDER_FALLBACK = "rgba(100, 116, 139, 0.32)";
const LANE_LABEL_FALLBACK = "rgba(226, 232, 240, 0.88)";
const LANE_LABEL_MUTED_FALLBACK = "rgba(148, 163, 184, 0.52)";

echarts.use([
	GridComponent,
	TooltipComponent,
	LineChart,
	CustomChart,
	SVGRenderer,
]);

type SharedIpUsageReport = Pick<
	AdminNodeIpUsageResponse,
	| "window_start"
	| "window_end"
	| "warnings"
	| "unique_ip_series"
	| "timeline"
	| "ips"
>;

type IpUsageViewProps = {
	title: string;
	description: string;
	window: AdminIpUsageWindow;
	onWindowChange: (window: AdminIpUsageWindow) => void;
	report: SharedIpUsageReport;
	isFetching?: boolean;
	emptyTitle?: string;
};

type TimeRange = {
	endMsExclusive: number;
	startMs: number;
};

type ActiveHighlight = {
	ip: string | null;
	hasFilter: boolean;
	timeRange: TimeRange | null;
};

type TimelineDatum = {
	endMinute: string;
	endMsExclusive: number;
	endpointTag: string;
	ip: string;
	laneKey: string;
	laneMinutes: number;
	startMinute: string;
	startMs: number;
	value: [laneIndex: number, startMs: number, endMsExclusive: number];
};

type ChartEventHandlers = NonNullable<EChartsReactProps["onEvents"]>;
type EChartsReactRef = InstanceType<typeof ReactEChartsCore>;
type TimelineTooltipTarget = {
	dataIndex: number;
};

function shouldRenderECharts(): boolean {
	if (typeof navigator === "undefined") return true;
	return !navigator.userAgent.toLowerCase().includes("jsdom");
}

function resolveCssColor(value: string, fallback: string): string {
	if (typeof document === "undefined" || !document.body) return fallback;
	const probe = document.createElement("span");
	probe.style.color = value;
	if (!probe.style.color) return fallback;
	probe.style.opacity = "0";
	probe.style.pointerEvents = "none";
	probe.style.position = "fixed";
	document.body.append(probe);
	const resolved = getComputedStyle(probe).color;
	probe.remove();
	return resolved || fallback;
}

function safeTimestamp(value: string, fallback = 0): number {
	const timestamp = new Date(value).getTime();
	return Number.isNaN(timestamp) ? fallback : timestamp;
}

function normalizeMinuteStart(timestamp: number): number {
	return Math.floor(timestamp / MINUTE_MS) * MINUTE_MS;
}

function buildTimeRange(startMs: number, endMsExclusive: number): TimeRange {
	const normalizedStart = normalizeMinuteStart(startMs);
	const normalizedEnd = Math.max(
		normalizedStart + MINUTE_MS,
		normalizeMinuteStart(endMsExclusive - 1) + MINUTE_MS,
	);
	return {
		endMsExclusive: normalizedEnd,
		startMs: normalizedStart,
	};
}

function buildMinuteRange(value: number | string): TimeRange {
	const minuteStart = normalizeMinuteStart(
		typeof value === "number" ? value : safeTimestamp(value),
	);
	return {
		endMsExclusive: minuteStart + MINUTE_MS,
		startMs: minuteStart,
	};
}

function sameTimeRange(a: TimeRange | null, b: TimeRange | null): boolean {
	if (!a || !b) return a === b;
	return a.startMs === b.startMs && a.endMsExclusive === b.endMsExclusive;
}

function rangesOverlap(a: TimeRange, b: TimeRange): boolean {
	return a.startMs < b.endMsExclusive && b.startMs < a.endMsExclusive;
}

function mergeTimeRanges(ranges: TimeRange[]): TimeRange[] {
	if (ranges.length === 0) return [];
	const sorted = [...ranges].sort(
		(left, right) => left.startMs - right.startMs,
	);
	const merged: TimeRange[] = [sorted[0] ?? buildMinuteRange(0)];
	for (const range of sorted.slice(1)) {
		const previous = merged.at(-1);
		if (!previous) {
			merged.push(range);
			continue;
		}
		if (range.startMs <= previous.endMsExclusive) {
			previous.endMsExclusive = Math.max(
				previous.endMsExclusive,
				range.endMsExclusive,
			);
			continue;
		}
		merged.push({ ...range });
	}
	return merged;
}

function formatDateTime(value: string): string {
	const dt = new Date(value);
	if (Number.isNaN(dt.getTime())) return value;
	return dt.toLocaleString();
}

function formatShortTime(value: string): string {
	const dt = new Date(value);
	if (Number.isNaN(dt.getTime())) return value;
	return dt.toLocaleString([], {
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
	});
}

function formatAxisTime(value: number, window: AdminIpUsageWindow): string {
	const dt = new Date(value);
	if (Number.isNaN(dt.getTime())) return "";
	if (window === "24h") {
		return dt.toLocaleTimeString([], {
			hour: "2-digit",
			minute: "2-digit",
		});
	}
	return dt.toLocaleString([], {
		month: "short",
		day: "numeric",
	});
}

function formatTimeRange(range: TimeRange): string {
	const inclusiveEnd = new Date(
		Math.max(range.startMs, range.endMsExclusive - MINUTE_MS),
	);
	return `${new Date(range.startMs).toLocaleString()} -> ${inclusiveEnd.toLocaleString()}`;
}

function isReportEmpty(report: SharedIpUsageReport): boolean {
	return report.ips.length === 0 && report.timeline.length === 0;
}

function hasWarning(
	warnings: AdminIpUsageWarning[],
	code: "online_stats_unavailable" | "geo_db_missing",
): boolean {
	return warnings.some((warning) => warning.code === code);
}

function isObjectRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null;
}

function getIpFromEvent(param: unknown): string | null {
	if (!isObjectRecord(param)) return null;
	const datum = param.data;
	if (isObjectRecord(datum) && typeof datum.ip === "string") return datum.ip;
	return null;
}

function getTimeRangeFromEvent(param: unknown): TimeRange | null {
	if (!isObjectRecord(param)) return null;
	const datum = param.data;
	if (isObjectRecord(datum)) {
		if (
			typeof datum.startMs === "number" &&
			typeof datum.endMsExclusive === "number"
		) {
			return buildTimeRange(datum.startMs, datum.endMsExclusive);
		}
		if (Array.isArray(datum.value) && typeof datum.value[0] === "number") {
			const endValue =
				typeof datum.value[1] === "number"
					? datum.value[1]
					: datum.value[0] + MINUTE_MS;
			return buildTimeRange(datum.value[0], endValue);
		}
	}
	if (Array.isArray(param.value) && typeof param.value[0] === "number") {
		const endValue =
			typeof param.value[1] === "number"
				? param.value[1]
				: param.value[0] + MINUTE_MS;
		return buildTimeRange(param.value[0], endValue);
	}
	return null;
}

function buildIpRangeIndex(
	lanes: AdminIpUsageTimelineLane[],
): Map<string, TimeRange[]> {
	const collected = new Map<string, TimeRange[]>();
	for (const lane of lanes) {
		const ranges = collected.get(lane.ip) ?? [];
		for (const segment of lane.segments) {
			ranges.push(
				buildTimeRange(
					safeTimestamp(segment.start_minute),
					safeTimestamp(segment.end_minute) + MINUTE_MS,
				),
			);
		}
		collected.set(lane.ip, ranges);
	}
	return new Map(
		Array.from(collected.entries(), ([ip, ranges]) => [
			ip,
			mergeTimeRanges(ranges),
		]),
	);
}

function ipMatchesTimeRange(
	ip: string,
	timeRange: TimeRange | null,
	ipRangeIndex: Map<string, TimeRange[]>,
): boolean {
	if (!timeRange) return true;
	const ranges = ipRangeIndex.get(ip) ?? [];
	return ranges.some((range) => rangesOverlap(range, timeRange));
}

function laneMatchesHighlight(
	lane: AdminIpUsageTimelineLane,
	activeHighlight: ActiveHighlight,
): boolean {
	if (!activeHighlight.hasFilter) return true;
	if (activeHighlight.ip && lane.ip !== activeHighlight.ip) return false;
	if (!activeHighlight.timeRange) return true;
	return lane.segments.some((segment) =>
		rangesOverlap(
			buildTimeRange(
				safeTimestamp(segment.start_minute),
				safeTimestamp(segment.end_minute) + MINUTE_MS,
			),
			activeHighlight.timeRange as TimeRange,
		),
	);
}

const renderVerticalBand: CustomSeriesRenderItem = (params, api) => {
	const start = api.coord([api.value(0), 0]);
	const end = api.coord([api.value(1), 0]);
	const coordSys = params.coordSys as unknown as {
		height: number;
		width: number;
		x: number;
		y: number;
	};
	const rectShape = echarts.graphic.clipRectByRect(
		{
			height: coordSys.height,
			width: Math.max(end[0] - start[0], 2),
			x: start[0],
			y: coordSys.y,
		},
		{
			height: coordSys.height,
			width: coordSys.width,
			x: coordSys.x,
			y: coordSys.y,
		},
	);
	if (!rectShape) return null;
	return {
		type: "rect",
		shape: rectShape,
		style: api.style(),
	};
};

function WindowSwitch({
	window,
	onWindowChange,
}: Pick<IpUsageViewProps, "window" | "onWindowChange">) {
	return (
		<div className="join">
			{(["24h", "7d"] as const).map((option) => (
				<button
					key={option}
					type="button"
					className={`btn join-item btn-sm ${window === option ? "btn-primary" : "btn-ghost"}`}
					onClick={() => onWindowChange(option)}
				>
					{option}
				</button>
			))}
		</div>
	);
}

function ChartSurface({
	ariaLabel,
	chartRef,
	height,
	onEvents,
	option,
}: {
	ariaLabel: string;
	chartRef?: { current: EChartsReactRef | null };
	height: number;
	onEvents?: ChartEventHandlers;
	option: EChartsOption;
}) {
	if (!shouldRenderECharts()) {
		return (
			<div
				role="img"
				aria-label={ariaLabel}
				className="w-full rounded-xl border border-base-300 bg-base-100/80"
				style={{ height }}
			/>
		);
	}

	return (
		<ReactEChartsCore
			ref={chartRef}
			echarts={echarts}
			option={option}
			notMerge
			lazyUpdate
			autoResize
			onEvents={onEvents}
			opts={SVG_RENDERER}
			style={{ height, width: "100%" }}
		/>
	);
}

function HighlightSummary({
	activeHighlight,
	hasPinnedHighlight,
	onClearPinned,
}: {
	activeHighlight: ActiveHighlight;
	hasPinnedHighlight: boolean;
	onClearPinned: () => void;
}) {
	return (
		<div className="flex flex-wrap items-center gap-2 rounded-xl border border-base-300 bg-base-200/40 px-3 py-2 text-xs">
			<span className="opacity-70">
				Hover or click any IP or time to highlight matching data across the
				chart and table.
			</span>
			{activeHighlight.ip ? (
				<span className="badge badge-info badge-outline gap-1">
					<span className="opacity-70">IP</span>
					<span className="font-mono">{activeHighlight.ip}</span>
				</span>
			) : null}
			{activeHighlight.timeRange ? (
				<span className="badge badge-warning badge-outline gap-1">
					<span className="opacity-70">Time</span>
					<span>{formatTimeRange(activeHighlight.timeRange)}</span>
				</span>
			) : null}
			{hasPinnedHighlight ? (
				<button
					type="button"
					className="btn btn-ghost btn-xs"
					onClick={onClearPinned}
				>
					Clear pinned highlight
				</button>
			) : null}
		</div>
	);
}

function UniqueIpAreaChart({
	report,
	window,
	activeHighlight,
	ipRangeIndex,
	onClearHover,
	onHoverTimeRange,
	onSelectTimeRange,
}: {
	activeHighlight: ActiveHighlight;
	ipRangeIndex: Map<string, TimeRange[]>;
	onClearHover: () => void;
	onHoverTimeRange: (timeRange: TimeRange | null) => void;
	onSelectTimeRange: (timeRange: TimeRange) => void;
	report: SharedIpUsageReport;
	window: AdminIpUsageWindow;
}) {
	const maxCount = Math.max(
		1,
		...report.unique_ip_series.map((point) => point.count),
	);
	const windowStartMs = safeTimestamp(report.window_start);
	const windowEndMs = safeTimestamp(
		report.window_end,
		report.unique_ip_series.at(-1)
			? safeTimestamp(
					report.unique_ip_series.at(-1)?.minute ?? report.window_end,
				)
			: windowStartMs,
	);
	const palette = useMemo(
		() => ({
			areaEnd: resolveCssColor(
				"color-mix(in srgb, var(--color-primary) 6%, transparent)",
				CHART_AREA_END_FALLBACK,
			),
			areaStart: resolveCssColor(
				"color-mix(in srgb, var(--color-primary) 34%, transparent)",
				CHART_AREA_START_FALLBACK,
			),
			ipBand: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 16%, transparent)",
				HIGHLIGHT_IP_BAND_FALLBACK,
			),
			line: resolveCssColor("var(--color-primary)", CHART_LINE_FALLBACK),
			timeBand: resolveCssColor(
				"color-mix(in srgb, var(--color-warning) 18%, transparent)",
				HIGHLIGHT_TIME_BAND_FALLBACK,
			),
			timePoint: resolveCssColor(
				"var(--color-warning)",
				HIGHLIGHT_TIME_POINT_FALLBACK,
			),
		}),
		[],
	);

	const activeIpBands = activeHighlight.ip
		? (ipRangeIndex.get(activeHighlight.ip) ?? []).map((range) => ({
				value: [range.startMs, range.endMsExclusive] as [number, number],
			}))
		: [];
	const activeTimeBand = activeHighlight.timeRange
		? [
				{
					value: [
						activeHighlight.timeRange.startMs,
						activeHighlight.timeRange.endMsExclusive,
					] as [number, number],
				},
			]
		: [];
	const activePoint = activeHighlight.timeRange
		? report.unique_ip_series.find(
				(point) =>
					normalizeMinuteStart(safeTimestamp(point.minute)) ===
					activeHighlight.timeRange?.startMs,
			)
		: null;

	const onEvents = useMemo<ChartEventHandlers>(
		() => ({
			click: (param: unknown) => {
				const nextRange = getTimeRangeFromEvent(param);
				if (nextRange) onSelectTimeRange(nextRange);
			},
			globalout: () => {
				onClearHover();
			},
			mousemove: (param: unknown) => {
				const nextRange = getTimeRangeFromEvent(param);
				if (nextRange) onHoverTimeRange(nextRange);
			},
		}),
		[onClearHover, onHoverTimeRange, onSelectTimeRange],
	);

	const option = useMemo(() => {
		const data = report.unique_ip_series.map((point) => [
			safeTimestamp(point.minute),
			point.count,
		]);
		const series: EChartsOption["series"] = [];
		if (activeIpBands.length > 0) {
			series.push({
				animation: false,
				data: activeIpBands,
				itemStyle: { color: palette.ipBand },
				renderItem: renderVerticalBand,
				silent: true,
				tooltip: { show: false },
				type: "custom",
				z: 0,
			});
		}
		if (activeTimeBand.length > 0) {
			series.push({
				animation: false,
				data: activeTimeBand,
				itemStyle: { color: palette.timeBand },
				renderItem: renderVerticalBand,
				silent: true,
				tooltip: { show: false },
				type: "custom",
				z: 1,
			});
		}
		series.push({
			animation: false,
			areaStyle: {
				color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
					{ offset: 0, color: palette.areaStart },
					{ offset: 1, color: palette.areaEnd },
				]),
				opacity: activeHighlight.hasFilter ? 0.9 : 1,
			},
			data,
			itemStyle: { color: palette.line },
			lineStyle: {
				color: palette.line,
				opacity: activeHighlight.hasFilter ? 0.92 : 1,
				width: activeHighlight.timeRange ? 2.4 : 2,
			},
			markPoint: activePoint
				? {
						animation: false,
						data: [
							{
								coord: [safeTimestamp(activePoint.minute), activePoint.count],
								name: "active-minute",
								itemStyle: {
									borderColor: "rgba(15, 23, 42, 0.68)",
									borderWidth: 2,
									color: palette.timePoint,
								},
								symbol: "circle",
								symbolSize: 10,
							},
						],
						label: { show: false },
						silent: true,
					}
				: undefined,
			showSymbol: false,
			smooth: 0.22,
			symbol: "none",
			type: "line",
			z: 2,
		});

		return {
			animation: false,
			grid: {
				bottom: 28,
				left: 42,
				right: 12,
				top: 14,
			},
			tooltip: {
				axisPointer: {
					lineStyle: {
						color: "rgba(148, 163, 184, 0.35)",
						type: "dashed",
					},
					type: "line",
				},
				formatter: (params: unknown) => {
					const point = Array.isArray(params) ? params.at(-1) : params;
					if (
						!point ||
						typeof point !== "object" ||
						!("value" in point) ||
						!Array.isArray(point.value)
					)
						return "";
					return [
						formatDateTime(new Date(Number(point.value[0])).toISOString()),
						`Unique IPs: ${point.value[1]}`,
					].join("<br/>");
				},
				trigger: "axis",
			},
			xAxis: {
				axisLabel: {
					color: CHART_AXIS_COLOR,
					hideOverlap: true,
					formatter: (value: number) => formatAxisTime(value, window),
				},
				axisLine: {
					lineStyle: { color: CHART_GRID_COLOR },
				},
				axisTick: { show: false },
				boundaryGap: false,
				max: windowEndMs,
				min: windowStartMs,
				splitLine: { show: false },
				type: "time",
			},
			yAxis: {
				axisLabel: {
					color: CHART_AXIS_COLOR,
				},
				axisLine: { show: false },
				axisTick: { show: false },
				max: maxCount,
				min: 0,
				minInterval: 1,
				splitLine: {
					lineStyle: { color: CHART_GRID_COLOR },
				},
				splitNumber: 3,
				type: "value",
			},
			series,
		} as unknown as EChartsOption;
	}, [
		activeHighlight.hasFilter,
		activeHighlight.timeRange,
		activeIpBands,
		activePoint,
		activeTimeBand,
		maxCount,
		palette.areaEnd,
		palette.areaStart,
		palette.ipBand,
		palette.line,
		palette.timeBand,
		palette.timePoint,
		report.unique_ip_series,
		window,
		windowEndMs,
		windowStartMs,
	]);

	return (
		<div className="rounded-2xl border border-base-300 bg-base-200/60 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">Unique IPs per minute</p>
					<p className="text-xs opacity-70">
						Exact minute series without downsampling.
					</p>
				</div>
				<div className="badge badge-outline">max {maxCount}</div>
			</div>
			<div className="rounded-xl border border-base-300 bg-base-100/80 p-1">
				<ChartSurface
					ariaLabel="Unique IPs per minute"
					height={AREA_CHART_HEIGHT}
					onEvents={onEvents}
					option={option}
				/>
			</div>
			<div className="mt-3 flex items-center justify-between text-xs opacity-70">
				<span>{formatShortTime(report.window_start)}</span>
				<span>{formatShortTime(report.window_end)}</span>
			</div>
		</div>
	);
}

function TimelineChart({
	lanes,
	window,
	windowStart,
	windowEnd,
	activeHighlight,
	onClearHover,
	onHoverSegment,
	onSelectSegment,
}: {
	activeHighlight: ActiveHighlight;
	lanes: AdminIpUsageTimelineLane[];
	onClearHover: () => void;
	onHoverSegment: (ip: string, timeRange: TimeRange) => void;
	onSelectSegment: (ip: string, timeRange: TimeRange) => void;
	window: AdminIpUsageWindow;
	windowEnd: string;
	windowStart: string;
}) {
	const timelineData = useMemo<TimelineDatum[]>(() => {
		return lanes.flatMap((lane, laneIndex) =>
			lane.segments.map((segment) => {
				const startMs = safeTimestamp(segment.start_minute);
				const endMsExclusive = safeTimestamp(segment.end_minute) + MINUTE_MS;
				return {
					endMinute: segment.end_minute,
					endMsExclusive,
					endpointTag: lane.endpoint_tag,
					ip: lane.ip,
					laneKey: lane.lane_key,
					laneMinutes: lane.minutes,
					startMinute: segment.start_minute,
					startMs,
					value: [laneIndex, startMs, endMsExclusive],
				};
			}),
		);
	}, [lanes]);

	const windowStartMs = safeTimestamp(windowStart);
	const windowEndMs = safeTimestamp(windowEnd, windowStartMs) + MINUTE_MS;
	const chartHeight = Math.max(180, lanes.length * 22 + 54);
	const palette = useMemo(
		() => ({
			activeBorder: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 92%, white)",
				LANE_ACTIVE_BORDER_FALLBACK,
			),
			activeFill: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 42%, transparent)",
				LANE_ACTIVE_FILL_FALLBACK,
			),
			border: resolveCssColor("var(--color-info)", LANE_BORDER_FALLBACK),
			fill: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 26%, transparent)",
				LANE_FILL_FALLBACK,
			),
			label: resolveCssColor(
				"color-mix(in srgb, var(--color-base-content) 82%, transparent)",
				LANE_LABEL_FALLBACK,
			),
			labelMuted: resolveCssColor(
				"color-mix(in srgb, var(--color-base-content) 42%, transparent)",
				LANE_LABEL_MUTED_FALLBACK,
			),
			mutedBorder: resolveCssColor(
				"color-mix(in srgb, var(--color-base-content) 18%, transparent)",
				LANE_MUTED_BORDER_FALLBACK,
			),
			mutedFill: resolveCssColor(
				"color-mix(in srgb, var(--color-base-content) 12%, transparent)",
				LANE_MUTED_FILL_FALLBACK,
			),
			timeBand: resolveCssColor(
				"color-mix(in srgb, var(--color-warning) 14%, transparent)",
				HIGHLIGHT_TIME_BAND_FALLBACK,
			),
		}),
		[],
	);

	const highlightBand = activeHighlight.timeRange
		? [
				{
					value: [
						activeHighlight.timeRange.startMs,
						activeHighlight.timeRange.endMsExclusive,
					] as [number, number],
				},
			]
		: [];
	const chartRef = useRef<EChartsReactRef | null>(null);
	const hoveredTooltipRef = useRef<TimelineTooltipTarget | null>(null);
	const tooltipRafRef = useRef<number | null>(null);
	const timelineSeriesIndex = highlightBand.length > 0 ? 1 : 0;
	const scheduleTimelineTooltip = useCallback(
		(dataIndex: number) => {
			if (tooltipRafRef.current !== null) {
				cancelAnimationFrame(tooltipRafRef.current);
			}
			tooltipRafRef.current = requestAnimationFrame(() => {
				chartRef.current?.getEchartsInstance().dispatchAction({
					type: "showTip",
					seriesIndex: timelineSeriesIndex,
					dataIndex,
				});
				tooltipRafRef.current = null;
			});
		},
		[timelineSeriesIndex],
	);

	const onEvents = useMemo<ChartEventHandlers>(
		() => ({
			click: (param: unknown) => {
				const ip = getIpFromEvent(param);
				const timeRange = getTimeRangeFromEvent(param);
				if (
					isObjectRecord(param) &&
					typeof param.dataIndex === "number" &&
					ip &&
					timeRange
				) {
					hoveredTooltipRef.current = { dataIndex: param.dataIndex };
					onSelectSegment(ip, timeRange);
					scheduleTimelineTooltip(param.dataIndex);
				}
			},
			globalout: () => {
				hoveredTooltipRef.current = null;
				onClearHover();
			},
			mouseover: (param: unknown) => {
				const ip = getIpFromEvent(param);
				const timeRange = getTimeRangeFromEvent(param);
				if (
					isObjectRecord(param) &&
					typeof param.dataIndex === "number" &&
					ip &&
					timeRange
				) {
					hoveredTooltipRef.current = { dataIndex: param.dataIndex };
					onHoverSegment(ip, timeRange);
					scheduleTimelineTooltip(param.dataIndex);
				}
			},
		}),
		[onClearHover, onHoverSegment, onSelectSegment, scheduleTimelineTooltip],
	);

	const option = useMemo(() => {
		const renderLane: CustomSeriesRenderItem = (params, api) => {
			const datum = timelineData[params.dataIndex ?? 0];
			if (!datum) return null;
			const laneIndex = api.value(0);
			const start = api.coord([api.value(1), laneIndex]);
			const end = api.coord([api.value(2), laneIndex]);
			const bandSize =
				typeof api.size === "function" ? api.size([0, 1]) : [0, 12];
			const categoryHeight = Array.isArray(bandSize)
				? (bandSize[1] ?? 12)
				: bandSize;
			const barHeight = Math.max(categoryHeight * 0.52, 6);
			const coordSys = params.coordSys as unknown as {
				height: number;
				width: number;
				x: number;
				y: number;
			};
			const rectShape = echarts.graphic.clipRectByRect(
				{
					height: barHeight,
					width: Math.max(end[0] - start[0], 3),
					x: start[0],
					y: start[1] - barHeight / 2,
				},
				{
					height: coordSys.height,
					width: coordSys.width,
					x: coordSys.x,
					y: coordSys.y,
				},
			);
			if (!rectShape) return null;
			const matchesHighlight =
				(!activeHighlight.ip || datum.ip === activeHighlight.ip) &&
				(!activeHighlight.timeRange ||
					rangesOverlap(
						buildTimeRange(datum.startMs, datum.endMsExclusive),
						activeHighlight.timeRange,
					));
			const dimmed = activeHighlight.hasFilter && !matchesHighlight;
			const fill = dimmed
				? palette.mutedFill
				: matchesHighlight && activeHighlight.hasFilter
					? palette.activeFill
					: palette.fill;
			const stroke = dimmed
				? palette.mutedBorder
				: matchesHighlight && activeHighlight.hasFilter
					? palette.activeBorder
					: palette.border;

			return {
				type: "rect",
				shape: { ...rectShape, r: 4 },
				style: api.style({
					fill,
					opacity: dimmed ? 0.48 : 1,
					shadowBlur: dimmed ? 0 : 8,
					shadowColor: "rgba(15, 23, 42, 0.18)",
					stroke,
					lineWidth: matchesHighlight && activeHighlight.hasFilter ? 1.4 : 1,
				}),
			};
		};

		const axisRich = {
			active: {
				color: palette.label,
				fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
				fontSize: 11,
				fontWeight: 600,
			},
			base: {
				color: palette.label,
				fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
				fontSize: 11,
			},
			muted: {
				color: palette.labelMuted,
				fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
				fontSize: 11,
			},
		};

		const series: EChartsOption["series"] = [];
		if (highlightBand.length > 0) {
			series.push({
				animation: false,
				data: highlightBand,
				itemStyle: { color: palette.timeBand },
				renderItem: renderVerticalBand,
				silent: true,
				tooltip: { show: false },
				type: "custom",
				z: 0,
			});
		}
		series.push({
			data: timelineData,
			encode: {
				x: [1, 2],
				y: 0,
			},
			renderItem: renderLane,
			type: "custom",
			z: 2,
		});

		return {
			animation: false,
			grid: {
				bottom: 16,
				left: 118,
				right: 18,
				top: 28,
			},
			tooltip: {
				formatter: (param: unknown) => {
					const datum =
						typeof param === "object" && param !== null && "data" in param
							? (param.data as TimelineDatum | undefined)
							: undefined;
					if (!datum) return "";
					return [
						`<strong>${datum.ip}</strong>`,
						datum.endpointTag,
						`${formatDateTime(datum.startMinute)} -> ${formatDateTime(datum.endMinute)}`,
						`Lane total: ${datum.laneMinutes} min`,
					].join("<br/>");
				},
				trigger: "item",
			},
			xAxis: {
				axisLabel: {
					color: CHART_AXIS_COLOR,
					hideOverlap: true,
					fontSize: 10,
					formatter: (value: number) => formatAxisTime(value, window),
				},
				axisLine: {
					lineStyle: { color: CHART_GRID_COLOR },
				},
				axisTick: { show: false },
				max: windowEndMs,
				min: windowStartMs,
				position: "top",
				splitLine: {
					lineStyle: { color: CHART_GRID_COLOR, type: "dashed" },
					show: true,
				},
				splitNumber: window === "24h" ? 6 : 7,
				type: "time",
			},
			yAxis: {
				axisLabel: {
					formatter: (_value: string, index: number) => {
						const lane = lanes[index];
						if (!lane) return "";
						if (!activeHighlight.hasFilter) return `{base|${lane.ip}}`;
						return laneMatchesHighlight(lane, activeHighlight)
							? `{active|${lane.ip}}`
							: `{muted|${lane.ip}}`;
					},
					margin: 14,
					rich: axisRich,
				},
				axisLine: { show: false },
				axisTick: { show: false },
				data: lanes.map((lane) => lane.lane_key),
				inverse: true,
				splitLine: {
					lineStyle: { color: "rgba(148, 163, 184, 0.10)" },
					show: true,
				},
				type: "category",
			},
			series,
		} as unknown as EChartsOption;
	}, [
		activeHighlight,
		highlightBand,
		lanes,
		palette.activeBorder,
		palette.activeFill,
		palette.border,
		palette.fill,
		palette.label,
		palette.labelMuted,
		palette.mutedBorder,
		palette.mutedFill,
		palette.timeBand,
		timelineData,
		window,
		windowEndMs,
		windowStartMs,
	]);

	return (
		<div className="rounded-2xl border border-base-300 bg-base-200/60 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">IP occupancy lanes</p>
					<p className="text-xs opacity-70">
						Hover or click a lane segment to cross-highlight the same IP and
						time slice.
					</p>
				</div>
				<div className="badge badge-outline">{lanes.length} lanes</div>
			</div>
			{lanes.length === 0 ? (
				<div className="rounded-xl border border-dashed border-base-300 px-4 py-8 text-center text-sm opacity-70">
					No occupancy lanes in this window.
				</div>
			) : (
				<div className="rounded-xl border border-base-300 bg-base-100/80 p-1">
					<ChartSurface
						ariaLabel="IP occupancy lanes"
						chartRef={chartRef}
						height={chartHeight}
						onEvents={onEvents}
						option={option}
					/>
				</div>
			)}
		</div>
	);
}

function IpListTable({
	ips,
	activeHighlight,
	ipRangeIndex,
	onHoverIp,
	onHoverTimeRange,
	onLeaveIp,
	onLeaveTimeRange,
	onSelectIp,
	onSelectTimeRange,
	selectedIp,
	selectedTimeRange,
}: {
	activeHighlight: ActiveHighlight;
	ipRangeIndex: Map<string, TimeRange[]>;
	ips: AdminIpUsageListEntry[];
	onHoverIp: (ip: string | null) => void;
	onHoverTimeRange: (timeRange: TimeRange | null) => void;
	onLeaveIp: (ip: string) => void;
	onLeaveTimeRange: (timeRange: TimeRange) => void;
	onSelectIp: (ip: string) => void;
	onSelectTimeRange: (timeRange: TimeRange) => void;
	selectedIp: string | null;
	selectedTimeRange: TimeRange | null;
}) {
	return (
		<div className="rounded-2xl border border-base-300 bg-base-200/60 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">IP list</p>
					<p className="text-xs opacity-70">
						Hover or click the IP / last seen time to highlight the matching
						rows and chart regions.
					</p>
				</div>
				<div className="badge badge-outline">{ips.length} IPs</div>
			</div>
			<div className="overflow-x-auto rounded-xl border border-base-300 bg-base-100">
				<table className="table table-sm">
					<thead>
						<tr>
							<th>IP</th>
							<th>Minutes</th>
							<th>Endpoints</th>
							<th>Region</th>
							<th>Operator</th>
							<th>Last seen</th>
						</tr>
					</thead>
					<tbody>
						{ips.length === 0 ? (
							<tr>
								<td colSpan={6} className="py-8 text-center text-sm opacity-70">
									No IP rows in this window.
								</td>
							</tr>
						) : (
							ips.map((entry) => {
								const timeButtonRange = buildMinuteRange(entry.last_seen_at);
								const matchesIp =
									!activeHighlight.ip || activeHighlight.ip === entry.ip;
								const matchesTime = ipMatchesTimeRange(
									entry.ip,
									activeHighlight.timeRange,
									ipRangeIndex,
								);
								const activeRow = matchesIp && matchesTime;
								const dimmed = activeHighlight.hasFilter && !activeRow;
								const ipButtonActive = activeHighlight.ip === entry.ip;
								const timeButtonActive = sameTimeRange(
									activeHighlight.timeRange,
									timeButtonRange,
								);

								return (
									<tr
										key={entry.ip}
										className={`${dimmed ? "opacity-45" : "opacity-100"} ${activeRow && activeHighlight.hasFilter ? "bg-info/8" : ""} transition-opacity`}
									>
										<td>
											<button
												type="button"
												className={`rounded px-1.5 py-0.5 font-mono text-xs transition-colors ${ipButtonActive ? "bg-info/15 text-info" : "hover:bg-base-200"}`}
												onMouseEnter={() => onHoverIp(entry.ip)}
												onMouseLeave={() => onLeaveIp(entry.ip)}
												onFocus={() => onHoverIp(entry.ip)}
												onBlur={() => onLeaveIp(entry.ip)}
												onClick={() => onSelectIp(entry.ip)}
												aria-pressed={selectedIp === entry.ip}
											>
												{entry.ip}
											</button>
										</td>
										<td>{entry.minutes}</td>
										<td className="text-xs">
											{entry.endpoint_tags.join(", ") || "-"}
										</td>
										<td className="text-xs">{entry.region || "Unknown"}</td>
										<td className="text-xs">{entry.operator || "Unknown"}</td>
										<td className="text-xs">
											<button
												type="button"
												className={`rounded px-1.5 py-0.5 text-left transition-colors ${timeButtonActive ? "bg-warning/15 text-warning" : "hover:bg-base-200"}`}
												onMouseEnter={() => onHoverTimeRange(timeButtonRange)}
												onMouseLeave={() => onLeaveTimeRange(timeButtonRange)}
												onFocus={() => onHoverTimeRange(timeButtonRange)}
												onBlur={() => onLeaveTimeRange(timeButtonRange)}
												onClick={() => onSelectTimeRange(timeButtonRange)}
												aria-pressed={sameTimeRange(
													selectedTimeRange,
													timeButtonRange,
												)}
											>
												{formatDateTime(entry.last_seen_at)}
											</button>
										</td>
									</tr>
								);
							})
						)}
					</tbody>
				</table>
			</div>
		</div>
	);
}

function WarningList({ warnings }: { warnings: AdminIpUsageWarning[] }) {
	if (warnings.length === 0) return null;
	return (
		<div className="space-y-2">
			{warnings.map((warning) => (
				<div
					key={warning.code}
					className={`alert py-2 text-sm ${warning.code === "online_stats_unavailable" ? "alert-warning" : "alert-info"}`}
				>
					<span>{warning.message}</span>
				</div>
			))}
		</div>
	);
}

export function IpUsageView({
	title,
	description,
	window,
	onWindowChange,
	report,
	isFetching = false,
	emptyTitle = "No inbound IP data",
}: IpUsageViewProps) {
	const [hoveredIp, setHoveredIp] = useState<string | null>(null);
	const [selectedIp, setSelectedIp] = useState<string | null>(null);
	const [hoveredTimeRange, setHoveredTimeRange] = useState<TimeRange | null>(
		null,
	);
	const [selectedTimeRange, setSelectedTimeRange] = useState<TimeRange | null>(
		null,
	);

	const empty = isReportEmpty(report);
	const blockingWarning = hasWarning(
		report.warnings,
		"online_stats_unavailable",
	);
	const ipRangeIndex = useMemo(
		() => buildIpRangeIndex(report.timeline),
		[report.timeline],
	);
	const activeHighlight = useMemo<ActiveHighlight>(
		() => ({
			hasFilter: Boolean(
				hoveredIp || hoveredTimeRange || selectedIp || selectedTimeRange,
			),
			ip: hoveredIp ?? selectedIp,
			timeRange: hoveredTimeRange ?? selectedTimeRange,
		}),
		[hoveredIp, hoveredTimeRange, selectedIp, selectedTimeRange],
	);
	const hasPinnedHighlight = Boolean(selectedIp || selectedTimeRange);

	const setHoveredIpStable = useCallback((nextIp: string | null) => {
		setHoveredIp((current) => (current === nextIp ? current : nextIp));
	}, []);
	const setHoveredTimeRangeStable = useCallback(
		(nextTimeRange: TimeRange | null) => {
			setHoveredTimeRange((current) =>
				sameTimeRange(current, nextTimeRange) ? current : nextTimeRange,
			);
		},
		[],
	);
	const setHoveredSegmentStable = useCallback(
		(ip: string, timeRange: TimeRange) => {
			setHoveredIp((current) => (current === ip ? current : ip));
			setHoveredTimeRange((current) =>
				sameTimeRange(current, timeRange) ? current : timeRange,
			);
		},
		[],
	);

	const clearHoveredIp = useCallback((ip?: string) => {
		setHoveredIp((current) => (!ip || current === ip ? null : current));
	}, []);
	const clearHoveredTimeRange = useCallback((timeRange?: TimeRange) => {
		setHoveredTimeRange((current) =>
			!timeRange || sameTimeRange(current, timeRange) ? null : current,
		);
	}, []);
	const clearHover = useCallback(() => {
		setHoveredIp(null);
		setHoveredTimeRange(null);
	}, []);
	const clearPinnedHighlight = useCallback(() => {
		setSelectedIp(null);
		setSelectedTimeRange(null);
	}, []);

	const selectIp = useCallback((ip: string) => {
		setSelectedIp((current) => (current === ip ? null : ip));
	}, []);
	const selectTimeRange = useCallback((timeRange: TimeRange) => {
		setSelectedTimeRange((current) =>
			sameTimeRange(current, timeRange) ? null : timeRange,
		);
	}, []);
	const selectTimelineSegment = useCallback(
		(ip: string, timeRange: TimeRange) => {
			if (selectedIp === ip && sameTimeRange(selectedTimeRange, timeRange)) {
				setSelectedIp(null);
				setSelectedTimeRange(null);
				return;
			}
			setSelectedIp(ip);
			setSelectedTimeRange(timeRange);
		},
		[selectedIp, selectedTimeRange],
	);

	return (
		<div className="card bg-base-100 shadow">
			<div className="card-body space-y-4">
				<div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
					<div>
						<h2 className="card-title">{title}</h2>
						<p className="text-sm opacity-70">{description}</p>
						<p className="mt-1 text-xs opacity-60">
							Window: {formatDateTime(report.window_start)} {"->"}{" "}
							{formatDateTime(report.window_end)}
						</p>
					</div>
					<div className="flex items-center gap-2">
						{isFetching ? (
							<span className="badge badge-ghost">refreshing</span>
						) : null}
						<WindowSwitch window={window} onWindowChange={onWindowChange} />
					</div>
				</div>

				<WarningList warnings={report.warnings} />

				{blockingWarning && empty ? (
					<div className="rounded-2xl border border-warning/40 bg-warning/10 px-4 py-10 text-center">
						<p className="text-base font-semibold">
							Online snapshots are unavailable
						</p>
						<p className="mt-2 text-sm opacity-80">
							This node is not exposing `statsUserOnline`, so xp cannot collect
							per-minute inbound IP occupancy.
						</p>
					</div>
				) : empty ? (
					<div className="rounded-2xl border border-dashed border-base-300 px-4 py-10 text-center">
						<p className="text-base font-semibold">{emptyTitle}</p>
						<p className="mt-2 text-sm opacity-70">
							No unique IPs were seen in the selected time window.
						</p>
					</div>
				) : (
					<div className="space-y-4">
						<HighlightSummary
							activeHighlight={activeHighlight}
							hasPinnedHighlight={hasPinnedHighlight}
							onClearPinned={clearPinnedHighlight}
						/>
						<UniqueIpAreaChart
							activeHighlight={activeHighlight}
							ipRangeIndex={ipRangeIndex}
							onClearHover={clearHover}
							onHoverTimeRange={setHoveredTimeRangeStable}
							onSelectTimeRange={selectTimeRange}
							report={report}
							window={window}
						/>
						<TimelineChart
							activeHighlight={activeHighlight}
							lanes={report.timeline}
							onClearHover={clearHover}
							onHoverSegment={setHoveredSegmentStable}
							onSelectSegment={selectTimelineSegment}
							window={window}
							windowEnd={report.window_end}
							windowStart={report.window_start}
						/>
						<IpListTable
							activeHighlight={activeHighlight}
							ipRangeIndex={ipRangeIndex}
							ips={report.ips}
							onHoverIp={setHoveredIpStable}
							onHoverTimeRange={setHoveredTimeRangeStable}
							onLeaveIp={clearHoveredIp}
							onLeaveTimeRange={clearHoveredTimeRange}
							onSelectIp={selectIp}
							onSelectTimeRange={selectTimeRange}
							selectedIp={selectedIp}
							selectedTimeRange={selectedTimeRange}
						/>
					</div>
				)}
			</div>
		</div>
	);
}

export type IpUsageGroupReport = AdminUserIpUsageNodeGroup;
