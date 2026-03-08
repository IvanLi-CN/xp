import type { CustomSeriesRenderItem, EChartsOption } from "echarts";
import ReactEChartsCore from "echarts-for-react/lib/core";
import { CustomChart, LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { SVGRenderer } from "echarts/renderers";
import { useMemo } from "react";

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

const CHART_GRID_COLOR = "rgba(148, 163, 184, 0.14)";
const CHART_AXIS_COLOR = "rgba(148, 163, 184, 0.55)";
const CHART_LINE_FALLBACK = "rgb(34, 211, 238)";
const CHART_AREA_START_FALLBACK = "rgba(34, 211, 238, 0.32)";
const CHART_AREA_END_FALLBACK = "rgba(34, 211, 238, 0.05)";
const LANE_FILL_FALLBACK = "rgba(56, 189, 248, 0.26)";
const LANE_BORDER_FALLBACK = "rgba(103, 232, 249, 0.92)";
const LANE_LABEL_FALLBACK = "rgba(226, 232, 240, 0.88)";

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

type TimelineDatum = {
	endMinute: string;
	endpointTag: string;
	ip: string;
	laneMinutes: number;
	startMinute: string;
	value: [laneIndex: number, startMs: number, endMsExclusive: number];
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

function isReportEmpty(report: SharedIpUsageReport): boolean {
	return report.ips.length === 0 && report.timeline.length === 0;
}

function hasWarning(
	warnings: AdminIpUsageWarning[],
	code: "online_stats_unavailable" | "geo_db_missing",
): boolean {
	return warnings.some((warning) => warning.code === code);
}

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
	height,
	option,
}: {
	ariaLabel: string;
	height: number;
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
			echarts={echarts}
			option={option}
			notMerge
			lazyUpdate
			autoResize
			opts={SVG_RENDERER}
			style={{ height, width: "100%" }}
		/>
	);
}

function UniqueIpAreaChart({
	report,
	window,
}: {
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
			line: resolveCssColor("var(--color-primary)", CHART_LINE_FALLBACK),
		}),
		[],
	);

	const option = useMemo(() => {
		const data = report.unique_ip_series.map((point) => [
			safeTimestamp(point.minute),
			point.count,
		]);
		return {
			animation: false,
			grid: {
				bottom: 28,
				left: 42,
				right: 12,
				top: 14,
			},
			tooltip: {
				trigger: "axis",
				axisPointer: {
					lineStyle: {
						color: "rgba(148, 163, 184, 0.35)",
						type: "dashed",
					},
					type: "line",
				},
				formatter: (params: unknown) => {
					const point = Array.isArray(params) ? params[0] : params;
					if (
						typeof point !== "object" ||
						point === null ||
						!("value" in point) ||
						!Array.isArray(point.value)
					)
						return "";
					return [
						formatDateTime(new Date(Number(point.value[0])).toISOString()),
						`Unique IPs: ${point.value[1]}`,
					].join("<br/>");
				},
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
			series: [
				{
					areaStyle: {
						color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
							{ offset: 0, color: palette.areaStart },
							{ offset: 1, color: palette.areaEnd },
						]),
					},
					data,
					itemStyle: { color: palette.line },
					lineStyle: {
						color: palette.line,
						width: 2,
					},
					showSymbol: false,
					smooth: 0.22,
					symbol: "none",
					type: "line",
				},
			],
		} as unknown as EChartsOption;
	}, [
		maxCount,
		palette.areaEnd,
		palette.areaStart,
		palette.line,
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
}: {
	lanes: AdminIpUsageTimelineLane[];
	window: AdminIpUsageWindow;
	windowEnd: string;
	windowStart: string;
}) {
	const timelineData = useMemo<TimelineDatum[]>(() => {
		return lanes.flatMap((lane, laneIndex) =>
			lane.segments.map((segment) => ({
				endMinute: segment.end_minute,
				endpointTag: lane.endpoint_tag,
				ip: lane.ip,
				laneMinutes: lane.minutes,
				startMinute: segment.start_minute,
				value: [
					laneIndex,
					safeTimestamp(segment.start_minute),
					safeTimestamp(segment.end_minute) + 60_000,
				],
			})),
		);
	}, [lanes]);

	const windowStartMs = safeTimestamp(windowStart);
	const windowEndMs = safeTimestamp(windowEnd, windowStartMs) + 60_000;
	const chartHeight = Math.max(180, lanes.length * 22 + 54);

	const palette = useMemo(
		() => ({
			border: resolveCssColor("var(--color-info)", LANE_BORDER_FALLBACK),
			fill: resolveCssColor(
				"color-mix(in srgb, var(--color-info) 26%, transparent)",
				LANE_FILL_FALLBACK,
			),
			label: resolveCssColor(
				"color-mix(in srgb, var(--color-base-content) 82%, transparent)",
				LANE_LABEL_FALLBACK,
			),
		}),
		[],
	);

	const option = useMemo(() => {
		const renderLane: CustomSeriesRenderItem = (params, api) => {
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
			return {
				type: "rect",
				shape: { ...rectShape, r: 4 },
				style: api.style({
					fill: palette.fill,
					shadowBlur: 8,
					shadowColor: "rgba(15, 23, 42, 0.18)",
					stroke: palette.border,
					lineWidth: 1,
				}),
			};
		};

		return {
			animation: false,
			grid: {
				bottom: 16,
				left: 110,
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
						`${formatDateTime(datum.startMinute)} → ${formatDateTime(datum.endMinute)}`,
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
					color: palette.label,
					fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
					fontSize: 11,
					formatter: (_value: string, index: number) => lanes[index]?.ip ?? "",
					margin: 14,
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
			series: [
				{
					data: timelineData,
					encode: {
						x: [1, 2],
						y: 0,
					},
					renderItem: renderLane,
					type: "custom",
				},
			],
		} as unknown as EChartsOption;
	}, [
		lanes,
		palette.border,
		palette.fill,
		palette.label,
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
						Rendered with Apache ECharts as a swimlane timeline for the top 20
						endpoint/IP rows.
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
						height={chartHeight}
						option={option}
					/>
				</div>
			)}
		</div>
	);
}

function IpListTable({ ips }: { ips: AdminIpUsageListEntry[] }) {
	return (
		<div className="rounded-2xl border border-base-300 bg-base-200/60 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">IP list</p>
					<p className="text-xs opacity-70">
						Aggregated by IP with unique minutes inside the selected window.
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
							ips.map((entry) => (
								<tr key={entry.ip}>
									<td className="font-mono text-xs">{entry.ip}</td>
									<td>{entry.minutes}</td>
									<td className="text-xs">
										{entry.endpoint_tags.join(", ") || "-"}
									</td>
									<td className="text-xs">{entry.region || "Unknown"}</td>
									<td className="text-xs">{entry.operator || "Unknown"}</td>
									<td className="text-xs">
										{formatDateTime(entry.last_seen_at)}
									</td>
								</tr>
							))
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
	const empty = isReportEmpty(report);
	const blockingWarning = hasWarning(
		report.warnings,
		"online_stats_unavailable",
	);

	return (
		<div className="card bg-base-100 shadow">
			<div className="card-body space-y-4">
				<div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
					<div>
						<h2 className="card-title">{title}</h2>
						<p className="text-sm opacity-70">{description}</p>
						<p className="mt-1 text-xs opacity-60">
							Window: {formatDateTime(report.window_start)} →{" "}
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
						<UniqueIpAreaChart report={report} window={window} />
						<TimelineChart
							lanes={report.timeline}
							window={window}
							windowEnd={report.window_end}
							windowStart={report.window_start}
						/>
						<IpListTable ips={report.ips} />
					</div>
				)}
			</div>
		</div>
	);
}

export type IpUsageGroupReport = AdminUserIpUsageNodeGroup;
