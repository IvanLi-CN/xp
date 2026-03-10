import type { CustomSeriesRenderItem, EChartsOption } from "echarts";
import ReactEChartsCore from "echarts-for-react/lib/core";
import type { EChartsReactProps } from "echarts-for-react/lib/types";
import { CustomChart, LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { SVGRenderer } from "echarts/renderers";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { buttonVariants } from "@/components/ui/button";
import { cn } from "@/lib/utils";

import type {
	AdminIpGeoSource,
	AdminIpUsageListEntry,
	AdminIpUsageTimelineLane,
	AdminIpUsageWarning,
	AdminIpUsageWindow,
	AdminNodeIpUsageResponse,
	AdminUserIpUsageNodeGroup,
} from "../api/adminIpUsage";
import { alertClass, badgeClass, tableClass } from "./ui-helpers";

const SVG_RENDERER = { renderer: "svg" } as const;
const AREA_CHART_HEIGHT = 224;
const MINUTE_MS = 60_000;
const HIGHLIGHT_TRANSITION_MS = 240;
const HIGHLIGHT_EASING = "cubicInOut";
const TIMELINE_CHART_GRID = {
	bottom: 16,
	left: 118,
	right: 18,
	top: 28,
} as const;

const CHART_GRID_COLOR = "rgba(148, 163, 184, 0.14)";
const CHART_AXIS_COLOR = "rgba(148, 163, 184, 0.55)";
const CHART_LINE_FALLBACK = "rgb(34, 211, 238)";
const CHART_AREA_START_FALLBACK = "rgba(34, 211, 238, 0.32)";
const CHART_AREA_END_FALLBACK = "rgba(34, 211, 238, 0.05)";
const HIGHLIGHT_IP_BAND_FALLBACK = "rgba(103, 232, 249, 0.10)";
const HIGHLIGHT_TIME_BAND_FALLBACK = "rgba(251, 191, 36, 0.14)";
const HIGHLIGHT_TIME_POINT_FALLBACK = "rgb(251, 191, 36)";
const HIGHLIGHT_TIME_RULE_FALLBACK = "rgba(251, 191, 36, 0.92)";
const LANE_FILL_FALLBACK = "rgba(56, 189, 248, 0.24)";
const LANE_BORDER_FALLBACK = "rgba(103, 232, 249, 0.92)";
const LANE_ACTIVE_FILL_FALLBACK = "rgba(34, 211, 238, 0.34)";
const LANE_ACTIVE_BORDER_FALLBACK = "rgba(165, 243, 252, 1)";
const LANE_MUTED_FILL_FALLBACK = "rgba(71, 85, 105, 0.20)";
const LANE_MUTED_BORDER_FALLBACK = "rgba(100, 116, 139, 0.40)";
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
	geoSource?: AdminIpGeoSource;
	onWindowChange: (window: AdminIpUsageWindow) => void;
	report: SharedIpUsageReport;
	isFetching?: boolean;
	emptyTitle?: string;
};

type TimeRange = {
	endMsExclusive: number;
	startMs: number;
};

type HighlightSource = "unique-chart" | "timeline-chart" | "ip-list";

type TimeHighlight = {
	range: TimeRange | null;
	source: HighlightSource | null;
};

type ActiveHighlight = {
	ip: string | null;
	hasFilter: boolean;
	timeRange: TimeRange | null;
	timeSource: HighlightSource | null;
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

type EChartsReactRef = InstanceType<typeof ReactEChartsCore>;
type EChartsInstance = ReturnType<EChartsReactRef["getEchartsInstance"]>;
type ChartEventHandlers = NonNullable<EChartsReactProps["onEvents"]>;
type TimelineTooltipTarget = {
	dataIndex: number;
};

function shouldRenderECharts(): boolean {
	if (typeof navigator === "undefined") return true;
	return !navigator.userAgent.toLowerCase().includes("jsdom");
}

let colorProbeContext: CanvasRenderingContext2D | null | undefined;

function normalizeCssColor(value: string): string {
	if (typeof document === "undefined") return value;
	if (
		typeof navigator !== "undefined" &&
		navigator.userAgent.toLowerCase().includes("jsdom")
	) {
		return value;
	}
	if (colorProbeContext === undefined) {
		try {
			const canvas = document.createElement("canvas");
			canvas.width = 1;
			canvas.height = 1;
			colorProbeContext = canvas.getContext("2d");
		} catch {
			colorProbeContext = null;
		}
	}
	const context = colorProbeContext;
	if (!context) return value;
	context.clearRect(0, 0, 1, 1);
	context.fillStyle = "rgba(0, 0, 0, 0)";
	context.fillRect(0, 0, 1, 1);
	context.fillStyle = value;
	context.fillRect(0, 0, 1, 1);
	const [red, green, blue, alpha] = context.getImageData(0, 0, 1, 1).data;
	if (alpha === 255) return `rgb(${red}, ${green}, ${blue})`;
	return `rgba(${red}, ${green}, ${blue}, ${Number((alpha / 255).toFixed(3))})`;
}

function resolveCssColor(value: string, fallback: string): string {
	if (typeof document === "undefined" || !document.body) return fallback;
	const probe = document.createElement("span");
	probe.style.color = value;
	if (!probe.style.color) return normalizeCssColor(fallback);
	probe.style.opacity = "0";
	probe.style.pointerEvents = "none";
	probe.style.position = "fixed";
	document.body.append(probe);
	const resolved = getComputedStyle(probe).color;
	probe.remove();
	return normalizeCssColor(resolved || fallback);
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
	code: "online_stats_unavailable",
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
				typeof datum.value[1] === "number" && datum.value[1] > datum.value[0]
					? datum.value[1]
					: datum.value[0] + MINUTE_MS;
			return buildTimeRange(datum.value[0], endValue);
		}
	}
	if (Array.isArray(param.value) && typeof param.value[0] === "number") {
		const endValue =
			typeof param.value[1] === "number" && param.value[1] > param.value[0]
				? param.value[1]
				: param.value[0] + MINUTE_MS;
		return buildTimeRange(param.value[0], endValue);
	}
	return null;
}

function getTimeRangeFromAxisPointerEvent(param: unknown): TimeRange | null {
	if (!isObjectRecord(param) || !Array.isArray(param.axesInfo)) return null;
	const axisInfo = param.axesInfo[0];
	if (!isObjectRecord(axisInfo)) return null;
	const value = axisInfo.value;
	if (typeof value !== "number" && typeof value !== "string") return null;
	return buildMinuteRange(value);
}

function getTimeRangeFromChartPointer(
	chart: EChartsInstance,
	point: { offsetX?: unknown; offsetY?: unknown },
	windowStartMs: number,
	windowEndMs: number,
): TimeRange | null {
	const offsetX = typeof point.offsetX === "number" ? point.offsetX : null;
	const offsetY = typeof point.offsetY === "number" ? point.offsetY : null;
	if (offsetX === null || offsetY === null) return null;
	if (!chart.containPixel({ gridIndex: 0 }, [offsetX, offsetY])) return null;
	const coord = chart.convertFromPixel({ xAxisIndex: 0 }, [offsetX, offsetY]);
	const axisValue = Array.isArray(coord) ? coord[0] : coord;
	if (typeof axisValue !== "number" || Number.isNaN(axisValue)) return null;
	const clampedValue = Math.min(
		windowEndMs,
		Math.max(windowStartMs, axisValue),
	);
	return buildMinuteRange(clampedValue);
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
		style: {
			fill:
				(api.visual("color") as string | undefined) ??
				"rgba(56, 189, 248, 0.12)",
			opacity: 1,
		},
		transition: ["shape", "style"],
		enterFrom: {
			shape: {
				...rectShape,
				width: 0,
				x: rectShape.x + rectShape.width / 2,
			},
			style: { opacity: 0 },
		},
		leaveTo: {
			shape: {
				...rectShape,
				width: 0,
				x: rectShape.x + rectShape.width / 2,
			},
			style: { opacity: 0 },
		},
	};
};

function WindowSwitch({
	window,
	onWindowChange,
}: Pick<IpUsageViewProps, "window" | "onWindowChange">) {
	return (
		<div className="flex flex-wrap items-center gap-1 rounded-xl border border-border/70 bg-muted/30 p-1">
			{(["24h", "7d"] as const).map((option) => (
				<button
					key={option}
					type="button"
					className={buttonVariants({
						variant: window === option ? "default" : "ghost",
						size: "sm",
					})}
					aria-pressed={window === option}
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
	notMerge = true,
	onEvents,
	option,
	replaceMerge,
}: {
	ariaLabel: string;
	chartRef?: { current: EChartsReactRef | null };
	height: number;
	notMerge?: boolean;
	onEvents?: ChartEventHandlers;
	option: EChartsOption;
	replaceMerge?: string[];
}) {
	if (!shouldRenderECharts()) {
		return (
			<div
				role="img"
				aria-label={ariaLabel}
				className="w-full rounded-xl border border-border/70 bg-card/80"
				style={{ height }}
			/>
		);
	}

	return (
		<ReactEChartsCore
			ref={chartRef}
			echarts={echarts}
			option={option}
			notMerge={notMerge}
			replaceMerge={replaceMerge}
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
		<div className="grid min-h-10 gap-2 rounded-xl border border-border/70 bg-muted/30 px-3 py-1.5 text-xs md:grid-cols-[minmax(0,1fr)_auto] md:items-center">
			<span className="min-w-0 opacity-70">
				Hover or click any IP or time to highlight matching data across the
				chart and table.
			</span>
			<div className="flex min-h-6 flex-wrap items-center gap-2 md:justify-self-end">
				{activeHighlight.ip ? (
					<span className={badgeClass("info", "default", "gap-1")}>
						<span className="opacity-70">IP</span>
						<span className="font-mono">{activeHighlight.ip}</span>
					</span>
				) : null}
				{activeHighlight.timeRange ? (
					<span className={badgeClass("warning", "default", "gap-1")}>
						<span className="opacity-70">Time</span>
						<span>{formatTimeRange(activeHighlight.timeRange)}</span>
					</span>
				) : null}
				{hasPinnedHighlight ? (
					<button
						type="button"
						className={cn(
							buttonVariants({ variant: "ghost", size: "sm" }),
							"h-7 px-2 text-[11px]",
						)}
						onClick={onClearPinned}
					>
						Clear pinned highlight
					</button>
				) : null}
			</div>
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
				id: `ip-band-${range.startMs}-${range.endMsExclusive}`,
				value: [range.startMs, range.endMsExclusive] as [number, number],
			}))
		: [];
	const activePoint = activeHighlight.timeRange
		? report.unique_ip_series.find(
				(point) =>
					normalizeMinuteStart(safeTimestamp(point.minute)) ===
					activeHighlight.timeRange?.startMs,
			)
		: null;
	const chartFrameRef = useRef<HTMLDivElement | null>(null);
	const [chartFrameWidth, setChartFrameWidth] = useState(0);

	useEffect(() => {
		const node = chartFrameRef.current;
		if (!node) return;

		const updateSize = () => {
			const nextWidth = node.getBoundingClientRect().width;
			setChartFrameWidth((current) =>
				Math.abs(current - nextWidth) < 0.5 ? current : nextWidth,
			);
		};

		updateSize();
		if (typeof ResizeObserver === "undefined") return;
		const observer = new ResizeObserver(() => {
			updateSize();
		});
		observer.observe(node);
		return () => {
			observer.disconnect();
		};
	}, []);

	const plotWidth = Math.max(chartFrameWidth - 42 - 12, 0);
	const plotHeight = AREA_CHART_HEIGHT - 14 - 28;
	const windowSpanMs = Math.max(windowEndMs - windowStartMs, MINUTE_MS);
	const timeBandOverlay = useMemo(() => {
		if (!activeHighlight.timeRange || plotWidth <= 0) return null;
		const startRatio = Math.min(
			1,
			Math.max(
				0,
				(activeHighlight.timeRange.startMs - windowStartMs) / windowSpanMs,
			),
		);
		const endRatio = Math.min(
			1,
			Math.max(
				startRatio,
				(activeHighlight.timeRange.endMsExclusive - windowStartMs) /
					windowSpanMs,
			),
		);
		return {
			left: 42 + startRatio * plotWidth,
			width: Math.max((endRatio - startRatio) * plotWidth, 2),
		};
	}, [activeHighlight.timeRange, plotWidth, windowSpanMs, windowStartMs]);
	const activePointOverlay = useMemo(() => {
		if (!activePoint || plotWidth <= 0) return null;
		const xRatio = Math.min(
			1,
			Math.max(
				0,
				(safeTimestamp(activePoint.minute) - windowStartMs) / windowSpanMs,
			),
		);
		const yRatio = Math.min(1, Math.max(0, activePoint.count / maxCount));
		return {
			left: 42 + xRatio * plotWidth,
			top: 14 + (1 - yRatio) * plotHeight,
		};
	}, [
		activePoint,
		maxCount,
		plotHeight,
		plotWidth,
		windowSpanMs,
		windowStartMs,
	]);

	const onEvents = useMemo<ChartEventHandlers>(
		() => ({
			click: (param: unknown, chart: EChartsInstance) => {
				const nextRange =
					(isObjectRecord(param) && isObjectRecord(param.event)
						? getTimeRangeFromChartPointer(
								chart,
								param.event,
								windowStartMs,
								windowEndMs,
							)
						: null) ?? getTimeRangeFromEvent(param);
				if (nextRange) onSelectTimeRange(nextRange);
			},
			globalout: () => {
				onClearHover();
			},
			updateAxisPointer: (param: unknown) => {
				const nextRange = getTimeRangeFromAxisPointerEvent(param);
				if (nextRange) {
					onHoverTimeRange(nextRange);
					return;
				}
				onClearHover();
			},
		}),
		[
			onClearHover,
			onHoverTimeRange,
			onSelectTimeRange,
			windowEndMs,
			windowStartMs,
		],
	);

	const option = useMemo(() => {
		const data = report.unique_ip_series.map((point) => [
			safeTimestamp(point.minute),
			point.count,
		]);
		const series: EChartsOption["series"] = [
			{
				id: "unique-ip-band",
				animationDuration: 0,
				animationDurationUpdate: HIGHLIGHT_TRANSITION_MS,
				animationEasingUpdate: HIGHLIGHT_EASING,
				data: activeIpBands,
				itemStyle: { color: palette.ipBand },
				renderItem: renderVerticalBand,
				silent: true,
				tooltip: { show: false },
				type: "custom",
				z: 0,
			},
			{
				id: "unique-line",
				animationDuration: 0,
				animationDurationUpdate: HIGHLIGHT_TRANSITION_MS,
				animationEasingUpdate: HIGHLIGHT_EASING,
				areaStyle: {
					color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
						{ offset: 0, color: palette.areaStart },
						{ offset: 1, color: palette.areaEnd },
					]),
					opacity: 1,
				},
				data,
				emphasis: { disabled: true },
				itemStyle: { color: palette.line },
				lineStyle: {
					color: palette.line,
					opacity: 1,
					width: activeHighlight.timeRange ? 2.08 : 2,
				},
				showSymbol: false,
				smooth: 0.22,
				symbol: "none",
				type: "line",
				z: 2,
			},
		];

		return {
			animation: true,
			animationDuration: 0,
			animationDurationUpdate: HIGHLIGHT_TRANSITION_MS,
			animationEasingUpdate: HIGHLIGHT_EASING,
			stateAnimation: {
				duration: HIGHLIGHT_TRANSITION_MS,
				easing: HIGHLIGHT_EASING,
			},
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
		activeHighlight.timeRange,
		activeIpBands,
		maxCount,
		palette.areaEnd,
		palette.areaStart,
		palette.ipBand,
		palette.line,
		report.unique_ip_series,
		window,
		windowEndMs,
		windowStartMs,
	]);

	return (
		<div className="rounded-2xl border border-border/70 bg-muted/30 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">Unique IPs per minute</p>
					<p className="text-xs opacity-70">
						Exact minute series without downsampling.
					</p>
				</div>
				<div className={badgeClass("outline")}>max {maxCount}</div>
			</div>
			<div className="rounded-xl border border-border/70 bg-card/80 p-1">
				<div ref={chartFrameRef} className="relative">
					<ChartSurface
						ariaLabel="Unique IPs per minute"
						height={AREA_CHART_HEIGHT}
						notMerge={false}
						onEvents={onEvents}
						option={option}
					/>
					{timeBandOverlay ? (
						<div
							data-ip-usage-time-band="true"
							aria-hidden="true"
							className="pointer-events-none absolute rounded-[4px]"
							style={{
								backgroundColor: palette.timeBand,
								height: plotHeight,
								left: timeBandOverlay.left,
								opacity: 1,
								top: 14,
								transition: "opacity 140ms ease",
								width: timeBandOverlay.width,
								zIndex: 4,
							}}
						/>
					) : null}
					{activePointOverlay ? (
						<div
							data-ip-usage-time-point="true"
							aria-hidden="true"
							className="pointer-events-none absolute size-2.5 rounded-full"
							style={{
								backgroundColor: palette.timePoint,
								border: "2px solid rgba(15, 23, 42, 0.68)",
								left: activePointOverlay.left,
								opacity: 1,
								top: activePointOverlay.top,
								transform: "translate(-50%, -50%)",
								transition: "opacity 140ms ease",
								zIndex: 5,
							}}
						/>
					) : null}
				</div>
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
				"color-mix(in srgb, var(--color-info) 86%, white 14%)",
				LANE_ACTIVE_BORDER_FALLBACK,
			),
			activeFill: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 34%, transparent)",
				LANE_ACTIVE_FILL_FALLBACK,
			),
			border: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 80%, white 6%)",
				LANE_BORDER_FALLBACK,
			),
			fill: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 24%, transparent)",
				LANE_FILL_FALLBACK,
			),
			label: resolveCssColor(
				"color-mix(in srgb, var(--color-foreground) 82%, transparent)",
				LANE_LABEL_FALLBACK,
			),
			labelMuted: resolveCssColor(
				"color-mix(in srgb, var(--color-foreground) 42%, transparent)",
				LANE_LABEL_MUTED_FALLBACK,
			),
			mutedBorder: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 24%, var(--color-foreground) 16%)",
				LANE_MUTED_BORDER_FALLBACK,
			),
			mutedFill: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 14%, var(--color-muted) 26%)",
				LANE_MUTED_FILL_FALLBACK,
			),
			timeBand: resolveCssColor(
				"color-mix(in srgb, var(--color-warning) 14%, transparent)",
				HIGHLIGHT_TIME_BAND_FALLBACK,
			),
			timeRule: resolveCssColor(
				"color-mix(in srgb, var(--color-warning) 90%, transparent)",
				HIGHLIGHT_TIME_RULE_FALLBACK,
			),
		}),
		[],
	);

	const chartFrameRef = useRef<HTMLDivElement | null>(null);
	const [chartFrameWidth, setChartFrameWidth] = useState(0);
	const showTimelineMarker =
		Boolean(activeHighlight.timeRange) &&
		activeHighlight.timeSource !== "timeline-chart";
	const timelinePlotWidth = Math.max(
		chartFrameWidth - TIMELINE_CHART_GRID.left - TIMELINE_CHART_GRID.right,
		0,
	);
	const timelinePlotHeight = Math.max(
		chartHeight - TIMELINE_CHART_GRID.top - TIMELINE_CHART_GRID.bottom,
		0,
	);
	const windowSpanMs = Math.max(windowEndMs - windowStartMs, MINUTE_MS);
	const timelineBandOverlay = useMemo(() => {
		if (
			!activeHighlight.timeRange ||
			!showTimelineMarker ||
			timelinePlotWidth <= 0
		) {
			return null;
		}
		const startRatio = Math.min(
			1,
			Math.max(
				0,
				(activeHighlight.timeRange.startMs - windowStartMs) / windowSpanMs,
			),
		);
		const endRatio = Math.min(
			1,
			Math.max(
				startRatio,
				(activeHighlight.timeRange.endMsExclusive - windowStartMs) /
					windowSpanMs,
			),
		);
		return {
			left: TIMELINE_CHART_GRID.left + startRatio * timelinePlotWidth,
			width: Math.max((endRatio - startRatio) * timelinePlotWidth, 2),
		};
	}, [
		activeHighlight.timeRange,
		showTimelineMarker,
		timelinePlotWidth,
		windowSpanMs,
		windowStartMs,
	]);
	const timelineRuleOverlay = useMemo(() => {
		if (
			!activeHighlight.timeRange ||
			!showTimelineMarker ||
			timelinePlotWidth <= 0
		) {
			return null;
		}
		const startRatio = Math.min(
			1,
			Math.max(
				0,
				(activeHighlight.timeRange.startMs - windowStartMs) / windowSpanMs,
			),
		);
		return {
			left: TIMELINE_CHART_GRID.left + startRatio * timelinePlotWidth,
		};
	}, [
		activeHighlight.timeRange,
		showTimelineMarker,
		timelinePlotWidth,
		windowSpanMs,
		windowStartMs,
	]);

	useEffect(() => {
		const node = chartFrameRef.current;
		if (!node) return;

		const updateSize = () => {
			const nextWidth = node.getBoundingClientRect().width;
			setChartFrameWidth((current) =>
				Math.abs(current - nextWidth) < 0.5 ? current : nextWidth,
			);
		};

		updateSize();
		if (typeof ResizeObserver === "undefined") return;
		const observer = new ResizeObserver(() => {
			updateSize();
		});
		observer.observe(node);
		return () => {
			observer.disconnect();
		};
	}, []);

	const chartRef = useRef<EChartsReactRef | null>(null);
	const hoveredTooltipRef = useRef<TimelineTooltipTarget | null>(null);
	const tooltipRafRef = useRef<number | null>(null);
	const timelineSeriesIndex = 0;
	const scheduleTimelineTooltip = useCallback((dataIndex: number) => {
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
	}, []);

	const syncTimelinePointer = useCallback(
		(param: unknown, mode: "hover" | "select") => {
			const ip = getIpFromEvent(param);
			const timeRange = getTimeRangeFromEvent(param);
			if (
				!isObjectRecord(param) ||
				typeof param.dataIndex !== "number" ||
				!ip ||
				!timeRange
			) {
				return;
			}

			const tooltipChanged =
				hoveredTooltipRef.current?.dataIndex !== param.dataIndex;
			hoveredTooltipRef.current = { dataIndex: param.dataIndex };

			if (mode === "select") {
				onSelectSegment(ip, timeRange);
				scheduleTimelineTooltip(param.dataIndex);
				return;
			}

			onHoverSegment(ip, timeRange);
			if (tooltipChanged) {
				scheduleTimelineTooltip(param.dataIndex);
			}
		},
		[onHoverSegment, onSelectSegment, scheduleTimelineTooltip],
	);

	const onEvents = useMemo<ChartEventHandlers>(
		() => ({
			click: (param: unknown) => {
				syncTimelinePointer(param, "select");
			},
			globalout: () => {
				hoveredTooltipRef.current = null;
				onClearHover();
			},
			mousemove: (param: unknown) => {
				syncTimelinePointer(param, "hover");
			},
			mouseover: (param: unknown) => {
				syncTimelinePointer(param, "hover");
			},
		}),
		[onClearHover, syncTimelinePointer],
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
				style: {
					fill,
					opacity: 1,
					shadowBlur: matchesHighlight && activeHighlight.hasFilter ? 4 : 0,
					shadowColor: "rgba(15, 23, 42, 0.12)",
					stroke,
					lineWidth: matchesHighlight && activeHighlight.hasFilter ? 1.15 : 1,
				},
				transition: ["shape", "style"],
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

		const series: EChartsOption["series"] = [
			{
				id: "timeline-lanes",
				animationDuration: 0,
				animationDurationUpdate: HIGHLIGHT_TRANSITION_MS,
				animationEasingUpdate: HIGHLIGHT_EASING,
				data: timelineData,
				encode: {
					x: [1, 2],
					y: 0,
				},
				renderItem: renderLane,
				type: "custom",
				z: 2,
			},
		];

		return {
			animation: true,
			animationDuration: 0,
			animationDurationUpdate: HIGHLIGHT_TRANSITION_MS,
			animationEasingUpdate: HIGHLIGHT_EASING,
			stateAnimation: {
				duration: HIGHLIGHT_TRANSITION_MS,
				easing: HIGHLIGHT_EASING,
			},
			grid: TIMELINE_CHART_GRID,
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
		lanes,
		palette.activeBorder,
		palette.activeFill,
		palette.border,
		palette.fill,
		palette.label,
		palette.labelMuted,
		palette.mutedBorder,
		palette.mutedFill,
		timelineData,
		window,
		windowEndMs,
		windowStartMs,
	]);

	return (
		<div className="rounded-2xl border border-border/70 bg-muted/30 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">IP occupancy lanes</p>
					<p className="text-xs opacity-70">
						Hover or click a lane segment to cross-highlight the same IP and
						time slice.
					</p>
				</div>
				<div className={badgeClass("outline")}>{lanes.length} lanes</div>
			</div>
			{lanes.length === 0 ? (
				<div className="rounded-xl border border-dashed border-border/70 px-4 py-8 text-center text-sm text-muted-foreground">
					No occupancy lanes in this window.
				</div>
			) : (
				<div className="rounded-xl border border-border/70 bg-card/80 p-1">
					<div ref={chartFrameRef} className="relative">
						<ChartSurface
							ariaLabel="IP occupancy lanes"
							chartRef={chartRef}
							height={chartHeight}
							notMerge={false}
							onEvents={onEvents}
							option={option}
						/>
						{timelineBandOverlay ? (
							<div
								data-ip-usage-timeline-band="true"
								aria-hidden="true"
								className="pointer-events-none absolute rounded-[4px]"
								style={{
									backgroundColor: palette.timeBand,
									height: timelinePlotHeight,
									left: timelineBandOverlay.left,
									opacity: 1,
									top: TIMELINE_CHART_GRID.top,
									transition: "opacity 140ms ease",
									width: timelineBandOverlay.width,
									zIndex: 0,
								}}
							/>
						) : null}
						{timelineRuleOverlay ? (
							<div
								data-ip-usage-timeline-rule="true"
								aria-hidden="true"
								className="pointer-events-none absolute rounded-full"
								style={{
									backgroundColor: palette.timeRule,
									boxShadow: `0 0 0 1px ${palette.timeBand}`,
									height: timelinePlotHeight,
									left: timelineRuleOverlay.left,
									opacity: 1,
									top: TIMELINE_CHART_GRID.top,
									transform: "translateX(-1px)",
									transition: "opacity 140ms ease",
									width: 2,
									zIndex: 1,
								}}
							/>
						) : null}
					</div>
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
		<div className="rounded-2xl border border-border/70 bg-muted/30 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">IP list</p>
					<p className="text-xs opacity-70">
						Hover or click the IP / last seen time to highlight the matching
						rows and chart regions.
					</p>
				</div>
				<div className={badgeClass("outline")}>{ips.length} IPs</div>
			</div>
			<div className="overflow-x-auto rounded-xl border border-border/70 bg-card">
				<table className={tableClass(true)}>
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
												className={`rounded px-1.5 py-0.5 font-mono text-xs transition-colors ${ipButtonActive ? "bg-info/15 text-info" : "hover:bg-muted"}`}
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
												className={`rounded px-1.5 py-0.5 text-left transition-colors ${timeButtonActive ? "bg-warning/15 text-warning" : "hover:bg-muted"}`}
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
					className={alertClass(
						warning.code === "online_stats_unavailable" ? "warning" : "info",
					)}
				>
					<span>{warning.message}</span>
				</div>
			))}
		</div>
	);
}

function GeoSourceNotice({ geoSource }: { geoSource?: AdminIpGeoSource }) {
	if (!geoSource) return null;
	const message = (() => {
		switch (geoSource) {
			case "country_is":
				return "Geo enrichment uses the free country.is hosted API.";
			case "managed_dbip_lite":
				return "Geo enrichment uses legacy managed DB-IP Lite MMDB data.";
			case "external_override":
				return "Geo enrichment uses a legacy external MMDB override.";
			case "missing":
				return "Geo enrichment is disabled (set XP_IP_GEO_ENABLED=true to enable country.is lookups).";
		}
	})();
	return (
		<div className={alertClass("info", "py-2 text-sm")}>
			<span>{message}</span>
		</div>
	);
}

export function IpUsageView({
	title,
	description,
	window,
	geoSource,
	onWindowChange,
	report,
	isFetching = false,
	emptyTitle = "No inbound IP data",
}: IpUsageViewProps) {
	const [hoveredIp, setHoveredIp] = useState<string | null>(null);
	const [selectedIp, setSelectedIp] = useState<string | null>(null);
	const [hoveredTime, setHoveredTime] = useState<TimeHighlight>({
		range: null,
		source: null,
	});
	const [selectedTime, setSelectedTime] = useState<TimeHighlight>({
		range: null,
		source: null,
	});

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
				hoveredIp || hoveredTime.range || selectedIp || selectedTime.range,
			),
			ip: hoveredIp ?? selectedIp,
			timeRange: hoveredTime.range ?? selectedTime.range,
			timeSource: hoveredTime.range
				? hoveredTime.source
				: selectedTime.range
					? selectedTime.source
					: null,
		}),
		[hoveredIp, hoveredTime, selectedIp, selectedTime],
	);
	const hasPinnedHighlight = Boolean(selectedIp || selectedTime.range);

	const setHoveredIpStable = useCallback((nextIp: string | null) => {
		setHoveredIp((current) => (current === nextIp ? current : nextIp));
	}, []);
	const setHoveredTimeRangeStable = useCallback(
		(nextTimeRange: TimeRange | null, source: HighlightSource) => {
			setHoveredTime((current) => {
				const nextSource = nextTimeRange ? source : null;
				if (
					sameTimeRange(current.range, nextTimeRange) &&
					current.source === nextSource
				) {
					return current;
				}
				return { range: nextTimeRange, source: nextSource };
			});
		},
		[],
	);
	const setHoveredSegmentStable = useCallback(
		(ip: string, timeRange: TimeRange) => {
			setHoveredIp((current) => (current === ip ? current : ip));
			setHoveredTime((current) => {
				if (
					sameTimeRange(current.range, timeRange) &&
					current.source === "timeline-chart"
				) {
					return current;
				}
				return { range: timeRange, source: "timeline-chart" };
			});
		},
		[],
	);

	const clearHoveredIp = useCallback((ip?: string) => {
		setHoveredIp((current) => (!ip || current === ip ? null : current));
	}, []);
	const clearHoveredTimeRange = useCallback((timeRange?: TimeRange) => {
		setHoveredTime((current) =>
			!timeRange || sameTimeRange(current.range, timeRange)
				? { range: null, source: null }
				: current,
		);
	}, []);
	const clearHover = useCallback(() => {
		setHoveredIp(null);
		setHoveredTime({ range: null, source: null });
	}, []);
	const clearPinnedHighlight = useCallback(() => {
		setSelectedIp(null);
		setSelectedTime({ range: null, source: null });
	}, []);

	const selectIp = useCallback((ip: string) => {
		setSelectedIp((current) => (current === ip ? null : ip));
	}, []);
	const selectTimeRange = useCallback(
		(timeRange: TimeRange, source: HighlightSource) => {
			setSelectedTime((current) => {
				if (
					sameTimeRange(current.range, timeRange) &&
					current.source === source
				) {
					return { range: null, source: null };
				}
				return { range: timeRange, source };
			});
		},
		[],
	);
	const selectTimelineSegment = useCallback(
		(ip: string, timeRange: TimeRange) => {
			if (
				selectedIp === ip &&
				sameTimeRange(selectedTime.range, timeRange) &&
				selectedTime.source === "timeline-chart"
			) {
				setSelectedIp(null);
				setSelectedTime({ range: null, source: null });
				return;
			}
			setSelectedIp(ip);
			setSelectedTime({ range: timeRange, source: "timeline-chart" });
		},
		[selectedIp, selectedTime],
	);

	return (
		<div className="rounded-2xl border border-border/70 bg-card shadow-sm">
			<div className="space-y-4 p-6">
				<div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
					<div>
						<h2 className="text-lg font-semibold tracking-tight">{title}</h2>
						<p className="text-sm opacity-70">{description}</p>
						<p className="mt-1 text-xs opacity-60">
							Window: {formatDateTime(report.window_start)} {"->"}{" "}
							{formatDateTime(report.window_end)}
						</p>
					</div>
					<div className="flex items-center gap-2">
						{isFetching ? (
							<span className={badgeClass("ghost")}>refreshing</span>
						) : null}
						<WindowSwitch window={window} onWindowChange={onWindowChange} />
					</div>
				</div>

				<WarningList warnings={report.warnings} />
				<GeoSourceNotice geoSource={geoSource} />

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
					<div className="rounded-2xl border border-dashed border-border/70 px-4 py-10 text-center">
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
							onHoverTimeRange={(timeRange) =>
								setHoveredTimeRangeStable(timeRange, "unique-chart")
							}
							onSelectTimeRange={(timeRange) =>
								selectTimeRange(timeRange, "unique-chart")
							}
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
							onHoverTimeRange={(timeRange) =>
								setHoveredTimeRangeStable(timeRange, "ip-list")
							}
							onLeaveIp={clearHoveredIp}
							onLeaveTimeRange={clearHoveredTimeRange}
							onSelectIp={selectIp}
							onSelectTimeRange={(timeRange) =>
								selectTimeRange(timeRange, "ip-list")
							}
							selectedIp={selectedIp}
							selectedTimeRange={selectedTime.range}
						/>
					</div>
				)}
			</div>
		</div>
	);
}

export type IpUsageGroupReport = AdminUserIpUsageNodeGroup;
