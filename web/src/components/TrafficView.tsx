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
};

const CHART_GRID_COLOR =
	"color-mix(in srgb, var(--color-muted-foreground) 18%, transparent)";
const CHART_AXIS_COLOR =
	"color-mix(in srgb, var(--color-muted-foreground) 78%, transparent)";
const CHART_REFERENCE_COLOR =
	"color-mix(in srgb, var(--color-muted-foreground) 82%, transparent)";
const CHART_CURRENT_AREA =
	"color-mix(in srgb, var(--color-primary) 18%, transparent)";
const CHART_CURRENT_DAY =
	"color-mix(in srgb, var(--color-primary) 7%, transparent)";

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

function buildTooltip(
	report: TrafficReport,
	window: TrafficWindow,
): EChartsOption["tooltip"] {
	return {
		trigger: "axis",
		confine: true,
		formatter: (params) => {
			const items = Array.isArray(params) ? params : [params];
			const index = Number(items[0]?.dataIndex ?? 0);
			const current = report.current[index];
			const reference = report.reference?.[index];
			if (!current) return "";
			const row = (label: string, point: TrafficSeriesPoint | undefined) =>
				point
					? `<div><strong>${label}</strong> · up ${point.uplink_bytes == null ? "-" : formatQuotaBytesHuman(point.uplink_bytes)} · down ${point.downlink_bytes == null ? "-" : formatQuotaBytesHuman(point.downlink_bytes)} · total ${point.total_bytes == null ? "-" : formatQuotaBytesHuman(point.total_bytes)}</div>`
					: "";
			return `<div class="space-y-1"><div>${formatTimestamp(current.start_at)}</div>${row("Current", current)}${row(`Previous ${window}`, reference)}</div>`;
		},
	};
}

export function TrafficView({
	report,
	window,
	onWindowChange,
	isFetching,
	nodeSelector,
}: TrafficViewProps) {
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
						itemStyle: { color: CHART_CURRENT_DAY },
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
							color: CHART_REFERENCE_COLOR,
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
					color: CHART_AXIS_COLOR,
					formatter: (value: string, index: number) => {
						if (window === "31d") return value.slice(5, 10);
						if (index === 0 || index === categories.length - 1)
							return formatTimestamp(value).slice(11, 16);
						return index % 48 === 0 ? formatTimestamp(value).slice(11, 16) : "";
					},
				},
				axisLine: { lineStyle: { color: CHART_GRID_COLOR } },
				axisTick: { show: false },
			},
			yAxis: {
				type: "value",
				min: 0,
				axisLabel: {
					color: CHART_AXIS_COLOR,
					formatter: (value: number) => formatQuotaBytesHuman(value),
				},
				splitLine: { lineStyle: { color: CHART_GRID_COLOR } },
			},
			tooltip: buildTooltip(report, window),
			series: [
				{
					name: "Current",
					type: "line",
					data: currentData,
					step: "end",
					connectNulls: false,
					symbol: "none",
					lineStyle: { color: "var(--color-primary)", width: 2, join: "round" },
					areaStyle: { color: CHART_CURRENT_AREA },
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
								lineStyle: {
									color: CHART_REFERENCE_COLOR,
									width: 1.5,
									type: "dashed" as const,
								},
							},
						]
					: []),
			],
		};
	}, [currentDayIndex, report, window]);

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
					<ReactEChartsCore
						echarts={echarts}
						option={option}
						notMerge
						style={{ height: 320, width: "100%" }}
						opts={{ renderer: "svg" }}
					/>
				)}
			</div>
		</section>
	);
}
