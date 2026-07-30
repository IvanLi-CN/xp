import type { EChartsOption } from "echarts";
import ReactEChartsCore from "echarts-for-react/lib/core";
import { LineChart } from "echarts/charts";
import {
	GridComponent,
	MarkAreaComponent,
	MarkLineComponent,
	TooltipComponent,
} from "echarts/components";
import * as echarts from "echarts/core";
import { SVGRenderer } from "echarts/renderers";
import { type ReactNode, useMemo } from "react";

import type {
	TrafficReport,
	TrafficSeriesPoint,
	TrafficWindow,
} from "../api/adminTraffic";
import { formatQuotaBytesHuman } from "../utils/quota";
import { Button } from "./Button";
import { PageState } from "./PageState";
import {
	type EChartsThemePalette,
	STATIC_LINE_SERIES_EMPHASIS,
	createThemedTooltipSurface,
	escapeEChartsHtml,
	useEChartsThemePalette,
} from "./echarts-theme";
import { alertClass } from "./ui-helpers";

echarts.use([
	GridComponent,
	MarkAreaComponent,
	MarkLineComponent,
	TooltipComponent,
	LineChart,
	SVGRenderer,
]);

type TrafficViewProps = {
	report: TrafficReport;
	window: TrafficWindow;
	onWindowChange: (window: TrafficWindow) => void;
	isFetching?: boolean;
	nodeSelector?: ReactNode;
	tooltipPreviewIndex?: number | null;
};

function pointValue(point: TrafficSeriesPoint): number | null {
	return point.total_bytes ?? null;
}

function formatTimestamp(value: string | null | undefined): string {
	if (!value) return "No sample yet";
	const date = new Date(value);
	return Number.isNaN(date.getTime())
		? value
		: date.toISOString().replace("T", " ").replace(".000Z", " UTC");
}

function formatTrafficTooltipMetrics(point: TrafficSeriesPoint) {
	const value = (bytes: number | null | undefined) =>
		bytes == null ? "-" : formatQuotaBytesHuman(bytes);
	return {
		downlink: value(point.downlink_bytes),
		total: value(point.total_bytes),
		uplink: value(point.uplink_bytes),
	};
}

function buildTrafficTooltipRow(
	label: string,
	point: TrafficSeriesPoint | undefined,
	palette: EChartsThemePalette["tooltip"],
): string {
	if (!point) return "";
	const metrics = formatTrafficTooltipMetrics(point);
	return [
		`<div style="margin-top:6px;font-size:12px;line-height:1.35;color:${palette.foreground};">`,
		`<strong>${escapeEChartsHtml(label)}</strong>`,
		`<span style="color:${palette.muted};">`,
		` · up ${escapeEChartsHtml(metrics.uplink)}`,
		` · down ${escapeEChartsHtml(metrics.downlink)}`,
		` · total ${escapeEChartsHtml(metrics.total)}</span></div>`,
	].join("");
}

export function buildTrafficTooltipHtml(
	report: TrafficReport,
	window: TrafficWindow,
	palette: EChartsThemePalette["tooltip"],
	index: number,
): string {
	const current = report.current[index];
	const reference = report.reference?.[index];
	if (!current) return "";
	return [
		'<div style="min-width:240px;">',
		`<div style="font-size:12px;line-height:1.2;color:${palette.muted};">`,
		escapeEChartsHtml(formatTimestamp(current.start_at)),
		"</div>",
		buildTrafficTooltipRow("Current", current, palette),
		buildTrafficTooltipRow(`Previous ${window}`, reference, palette),
		"</div>",
	].join("");
}

function buildTooltip(
	report: TrafficReport,
	window: TrafficWindow,
	palette: EChartsThemePalette,
): EChartsOption["tooltip"] {
	return {
		...createThemedTooltipSurface(palette),
		trigger: "axis",
		confine: true,
		formatter: (params) => {
			const items = Array.isArray(params) ? params : [params];
			return buildTrafficTooltipHtml(
				report,
				window,
				palette.tooltip,
				Number(items[0]?.dataIndex ?? 0),
			);
		},
	};
}

function TrafficTooltipPreviewCard({
	report,
	window,
	palette,
	index,
}: {
	report: TrafficReport;
	window: TrafficWindow;
	palette: EChartsThemePalette["tooltip"];
	index: number;
}) {
	const current = report.current[index];
	if (!current) return null;
	const rows = [
		{ label: "Current", point: current },
		{ label: `Previous ${window}`, point: report.reference?.[index] },
	].filter(
		(row): row is { label: string; point: TrafficSeriesPoint } =>
			row.point != null,
	);

	return (
		<div
			className="pointer-events-none absolute right-4 top-4 z-10 max-w-[calc(100%-2rem)] rounded-xl border p-3 shadow-lg"
			data-testid="traffic-tooltip-preview"
			style={{
				backgroundColor: palette.background,
				borderColor: palette.border,
				boxShadow: `${palette.shadowOffsetX}px ${palette.shadowOffsetY}px ${palette.shadowBlur}px 0 ${palette.shadow}`,
				color: palette.foreground,
			}}
		>
			<div className="text-xs" style={{ color: palette.muted }}>
				{formatTimestamp(current.start_at)}
			</div>
			{rows.map(({ label, point }) => {
				const metrics = formatTrafficTooltipMetrics(point);
				return (
					<div className="mt-1.5 text-xs" key={label}>
						<strong>{label}</strong>
						<span style={{ color: palette.muted }}>
							{" "}
							· up {metrics.uplink} · down {metrics.downlink} · total{" "}
							{metrics.total}
						</span>
					</div>
				);
			})}
		</div>
	);
}

export function TrafficView({
	report,
	window,
	onWindowChange,
	isFetching,
	nodeSelector,
	tooltipPreviewIndex = null,
}: TrafficViewProps) {
	const palette = useEChartsThemePalette();
	const currentDayIndex = report.current.findIndex(
		(point) => point.is_current_day,
	);
	const option = useMemo<EChartsOption>(() => {
		const categories = report.current.map((point) => point.start_at);
		const currentData = report.current.map(pointValue);
		const referenceData = report.reference?.map(pointValue) ?? [];
		const currentDayStart =
			currentDayIndex >= 0 ? currentDayIndex - 0.5 : report.current.length;
		const currentDayEnd = report.current.length - 0.5;
		const markArea =
			window === "31d" && currentDayIndex >= 0
				? {
						silent: true,
						itemStyle: { color: palette.currentDay },
						data: [
							[{ xAxis: currentDayStart }, { xAxis: currentDayEnd }] as [
								{ xAxis: number },
								{ xAxis: number },
							],
						],
					}
				: undefined;
		const markLine =
			window === "31d" && currentDayIndex > 0
				? {
						silent: true,
						symbol: "none",
						lineStyle: {
							color: palette.reference,
							type: "dashed" as const,
							width: 1,
						},
						data: [{ xAxis: currentDayStart }],
					}
				: undefined;
		return {
			animation: false,
			grid: { left: 56, right: 18, top: 20, bottom: 32 },
			xAxis: {
				type: "category",
				boundaryGap: false,
				data: categories,
				axisLabel: {
					color: palette.axis,
					formatter: (value: string, index: number) => {
						if (window === "31d") return value.slice(5, 10);
						if (index === 0 || index === categories.length - 1)
							return formatTimestamp(value).slice(11, 16);
						return index % 48 === 0 ? formatTimestamp(value).slice(11, 16) : "";
					},
				},
				axisLine: { lineStyle: { color: palette.grid } },
				axisTick: { show: false },
			},
			yAxis: {
				type: "value",
				min: 0,
				axisLabel: {
					color: palette.axis,
					formatter: (value: number) => formatQuotaBytesHuman(value),
				},
				splitLine: { lineStyle: { color: palette.grid } },
			},
			tooltip: buildTooltip(report, window, palette),
			series: [
				{
					name: "Current",
					type: "line",
					data: currentData,
					step: "end",
					connectNulls: false,
					symbol: "none",
					emphasis: STATIC_LINE_SERIES_EMPHASIS,
					lineStyle: { color: palette.primary, width: 2, join: "round" },
					areaStyle: { color: palette.currentArea },
					markArea,
					markLine,
				},
				...(report.reference
					? [
							{
								name: "Previous",
								type: "line" as const,
								data: referenceData,
								step: "end" as const,
								connectNulls: false,
								symbol: "none",
								emphasis: STATIC_LINE_SERIES_EMPHASIS,
								lineStyle: {
									color: palette.reference,
									width: 1.5,
									type: "dashed" as const,
								},
							},
						]
					: []),
			],
		};
	}, [currentDayIndex, palette, report, window]);
	return (
		<section className="space-y-4">
			<div className="flex flex-wrap items-start justify-between gap-3">
				<div>
					<h2 className="xp-card-title">Traffic</h2>
					<p className="text-sm text-muted-foreground">
						Actual uplink and downlink bytes from Xray membership counters.
					</p>
				</div>
				<div className="flex flex-wrap items-center gap-2">
					{nodeSelector}
					<fieldset className="inline-flex rounded-xl border border-border/70 bg-muted/30 p-1">
						<legend className="sr-only">Traffic window</legend>
						{(["24h", "31d"] as const).map((value) => (
							<Button
								key={value}
								size="sm"
								variant={window === value ? "primary" : "ghost"}
								onClick={() => onWindowChange(value)}
							>
								{value === "24h" ? "Last 24 hours" : "Last 31 days"}
							</Button>
						))}
					</fieldset>
				</div>
			</div>

			<div className="grid gap-3 md:grid-cols-3">
				{[
					["Uplink", report.summary.uplink_bytes],
					["Downlink", report.summary.downlink_bytes],
					["Total", report.summary.total_bytes],
				].map(([label, value]) => (
					<div
						className="rounded-xl border border-border/70 bg-card px-4 py-3"
						key={label}
					>
						<div className="text-xs uppercase tracking-wide text-muted-foreground">
							{label}
						</div>
						<div className="mt-1 text-xl font-semibold">
							{formatQuotaBytesHuman(Number(value))}
						</div>
					</div>
				))}
			</div>

			<div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
				<span>
					{formatTimestamp(report.window_start_at)} →{" "}
					{formatTimestamp(report.window_end_at)} · UTC
				</span>
				<span>
					Last sample: {formatTimestamp(report.last_sample_at)}
					{isFetching ? " · refreshing" : ""}
				</span>
			</div>
			{report.partial ? (
				<div className={alertClass("warning", "py-2")}>
					Traffic data is partial. Missing samples remain gaps in the chart.
				</div>
			) : null}
			{report.warnings.length > 0 ? (
				<div className="text-xs text-muted-foreground">
					{report.warnings.slice(0, 2).join(" · ")}
				</div>
			) : null}
			<div className="rounded-xl border border-border/70 bg-card p-2 sm:p-4">
				{report.current.every((point) => point.total_bytes == null) &&
				(report.reference?.every((point) => point.total_bytes == null) ??
					true) ? (
					<PageState
						variant="empty"
						title="No traffic samples yet"
						description="The chart will populate after the first successful five-minute sample."
					/>
				) : (
					<div className="relative">
						<ReactEChartsCore
							echarts={echarts}
							option={option}
							notMerge
							style={{ height: 320, width: "100%" }}
							opts={{ renderer: "svg" }}
						/>
						{tooltipPreviewIndex !== null ? (
							<TrafficTooltipPreviewCard
								index={tooltipPreviewIndex}
								palette={palette.tooltip}
								report={report}
								window={window}
							/>
						) : null}
					</div>
				)}
			</div>
		</section>
	);
}
