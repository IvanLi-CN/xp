import type { EChartsOption } from "echarts";
import ReactEChartsCore from "echarts-for-react/lib/core";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { SVGRenderer } from "echarts/renderers";
import { useMemo } from "react";

import type {
	AdminNodeTcpConnectionsResponse,
	AdminTcpConnectionHistoryPoint,
} from "../api/adminTcpConnections";
import {
	STATIC_LINE_SERIES_EMPHASIS,
	createThemedTooltipSurface,
	escapeEChartsHtml,
	useEChartsThemePalette,
} from "./echarts-theme";
import { Alert } from "./ui/alert";
import { Badge } from "./ui/badge";

echarts.use([GridComponent, TooltipComponent, LineChart, SVGRenderer]);

const SVG_RENDERER = { renderer: "svg" } as const;
const CHART_HEIGHT = 260;

const CLASS_LABELS: Record<string, string> = {
	inbound: "Inbound",
	control_plane: "Control plane",
	outbound_runtime: "Runtime outbound",
	outbound_other: "Other outbound",
};

type Series = {
	name: string;
	points: Array<[number, number]>;
};

function shouldRenderECharts(): boolean {
	if (typeof navigator === "undefined") return true;
	return !navigator.userAgent.toLowerCase().includes("jsdom");
}

function toPoints(
	points: AdminTcpConnectionHistoryPoint[],
): Array<[number, number]> {
	return points.flatMap((point) => {
		const timestamp = new Date(point.bucket_start).getTime();
		return Number.isNaN(timestamp)
			? []
			: [[timestamp, point.max] as [number, number]];
	});
}

function qualityVariant(
	quality: NonNullable<AdminNodeTcpConnectionsResponse["quality"]>,
): "info" | "warning" | "ghost" {
	if (quality === "complete") return "ghost";
	if (quality === "unsupported") return "info";
	return "warning";
}

export type TcpConnectionHistoryReport = Pick<
	AdminNodeTcpConnectionsResponse,
	"window_start" | "window_end"
> &
	Partial<
		Pick<
			AdminNodeTcpConnectionsResponse,
			| "window"
			| "resolution"
			| "quality"
			| "reasons"
			| "truncated_buckets"
			| "partial_buckets"
			| "deleted_subjects"
			| "subjects_truncated"
			| "subjects_omitted"
			| "total"
			| "classes"
			| "subjects"
		>
	>;

export type TcpConnectionHistoryChartProps = {
	report: TcpConnectionHistoryReport;
	window: string;
};

export function TcpConnectionHistoryChart({
	report,
	window,
}: TcpConnectionHistoryChartProps) {
	const palette = useEChartsThemePalette();
	const classes = report.classes ?? [];
	const totalPoints = report.total?.points ?? [];
	const subjects = report.subjects ?? [];
	const quality = report.quality ?? "partial";
	const reasons = report.reasons ?? [];
	const truncatedBuckets = report.truncated_buckets ?? [];
	const partialBuckets = report.partial_buckets ?? [];
	const deletedSubjects = report.deleted_subjects ?? [];

	const series = useMemo<Series[]>(() => {
		const stacked = classes.map((entry) => ({
			name: CLASS_LABELS[entry.class] ?? entry.class,
			points: toPoints(entry.points),
		}));
		return [
			{ name: "Total", points: toPoints(totalPoints) },
			...stacked,
		].filter((item) => item.points.length > 0);
	}, [classes, totalPoints]);

	const current = totalPoints.at(-1)?.max ?? 0;
	const peak = totalPoints.reduce(
		(maximum, point) => Math.max(maximum, point.max),
		0,
	);

	const option = useMemo<EChartsOption>(() => {
		const colors = [
			palette.primary,
			palette.reference,
			palette.currentArea,
			palette.currentDay,
			palette.axis,
		];
		return {
			animationDuration: 220,
			color: colors,
			grid: { top: 32, right: 18, bottom: 36, left: 42 },
			legend: { show: false },
			tooltip: {
				...createThemedTooltipSurface(palette),
				trigger: "axis",
				confine: true,
				formatter: (params: unknown) => {
					const items = Array.isArray(params) ? params : [params];
					const first = items[0] as
						| { axisValue?: number | string; marker?: string }
						| undefined;
					const value =
						typeof first?.axisValue === "number"
							? first.axisValue
							: Number(first?.axisValue);
					if (!Number.isFinite(value)) return "";
					const heading = new Date(value).toISOString();
					const rows = items
						.map((item) => {
							const entry = item as {
								marker?: string;
								seriesName?: string;
								value?: [number, number];
							};
							const count = entry.value?.[1];
							if (typeof count !== "number") return "";
							const rowStyle = [
								"display:flex",
								"align-items:center",
								"justify-content:space-between",
								"gap:16px",
								"margin-top:6px",
							].join(";");
							const labelStyle = [
								"display:flex",
								"align-items:center",
								"gap:6px",
								"font-size:12px",
								`color:${palette.tooltip.muted}`,
							].join(";");
							const valueStyle = [
								"font-size:12px",
								"font-weight:600",
								`color:${palette.tooltip.foreground}`,
							].join(";");
							return `<div style="${rowStyle}">
								<span style="${labelStyle}">
									${entry.marker ?? ""}${escapeEChartsHtml(entry.seriesName ?? "")}
								</span>
								<span style="${valueStyle}">${count}</span>
							</div>`;
						})
						.join("");
					const headingStyle = `font-size:13px;color:${palette.tooltip.muted};`;
					return `<div style="width:min(260px,100%);">
						<div style="${headingStyle}">${escapeEChartsHtml(heading)}</div>
						${rows}
					</div>`;
				},
			},
			xAxis: {
				type: "time",
				min: new Date(report.window_start ?? "").getTime() || undefined,
				max: new Date(report.window_end ?? "").getTime() || undefined,
				axisLabel: { color: palette.axis, hideOverlap: true },
				axisLine: { lineStyle: { color: palette.grid } },
				axisTick: { show: false },
				splitLine: { show: false },
			},
			yAxis: {
				type: "value",
				minInterval: 1,
				axisLabel: { color: palette.axis },
				axisLine: { show: false },
				axisTick: { show: false },
				splitLine: { lineStyle: { color: palette.grid } },
			},
			series: series.map((item) => ({
				name: item.name,
				type: "line" as const,
				stack: item.name === "Total" ? undefined : "classes",
				step: "end" as const,
				smooth: false,
				showSymbol: false,
				emphasis: STATIC_LINE_SERIES_EMPHASIS,
				data: item.points,
				lineStyle: item.name === "Total" ? { width: 2 } : undefined,
				areaStyle:
					item.name === "Total" ? { opacity: 0.12 } : { opacity: 0.24 },
			})),
		};
	}, [palette, report.window_end, report.window_start, series]);

	return (
		<div className="space-y-3">
			<div className="flex flex-wrap items-center gap-2">
				<Badge variant="ghost" size="sm" className="font-normal">
					<span className="font-semibold text-foreground">{current}</span>
					current
				</Badge>
				<Badge variant="ghost" size="sm" className="font-normal">
					<span className="font-semibold text-foreground">{peak}</span>
					peak
				</Badge>
				<Badge variant="outline" size="sm" className="font-normal">
					{window} / {report.resolution ?? "1m"}
				</Badge>
				<Badge
					variant={qualityVariant(quality)}
					size="sm"
					className="font-normal"
				>
					{quality}
				</Badge>
				{(report.total?.approximate || classes.some((c) => c.approximate)) && (
					<Badge variant="ghost" size="sm" className="font-normal">
						approximate
					</Badge>
				)}
				{subjects.length > 0 ? (
					<Badge variant="ghost" size="sm" className="font-normal">
						<span className="font-semibold text-foreground">
							{subjects.length}
						</span>
						subjects
					</Badge>
				) : null}
			</div>

			{quality !== "complete" && reasons.length > 0 ? (
				<Alert tone="warning" className="py-2">
					<span>{reasons.join(", ")}</span>
				</Alert>
			) : null}
			{truncatedBuckets.length > 0 ? (
				<Alert tone="warning" className="py-2">
					<span>
						{truncatedBuckets.length} bucket
						{truncatedBuckets.length === 1 ? "" : "s"} hit the socket inspection
						limit; values are lower bounds.
					</span>
				</Alert>
			) : null}
			{partialBuckets.length > 0 ? (
				<Alert tone="info" className="py-2">
					<span>
						{partialBuckets.length} bucket
						{partialBuckets.length === 1 ? "" : "s"} are partial (
						{Array.from(
							new Set(partialBuckets.map((item) => item.reason)),
						).join(", ")}
						).
					</span>
				</Alert>
			) : null}
			{deletedSubjects.length > 0 ? (
				<Alert tone="info" className="py-2">
					<span>
						{deletedSubjects.length} deleted endpoint subject
						{deletedSubjects.length === 1 ? "" : "s"} hidden; node totals keep
						their historical value.
					</span>
				</Alert>
			) : null}
			{report.subjects_omitted ? (
				<Alert tone="info" className="py-2">
					<span>
						Subject detail was omitted at repository ingest to stay inside the
						per-record budget; totals and classes are complete.
					</span>
				</Alert>
			) : null}
			{report.subjects_truncated ? (
				<Alert tone="info" className="py-2">
					<span>
						Subject list is paginated; totals and classes are complete.
					</span>
				</Alert>
			) : null}

			{shouldRenderECharts() ? (
				<ReactEChartsCore
					echarts={echarts}
					option={option}
					notMerge
					lazyUpdate
					autoResize
					opts={SVG_RENDERER}
					style={{ height: CHART_HEIGHT, width: "100%" }}
				/>
			) : (
				<div
					role="img"
					aria-label={`Full-site TCP connections chart, current ${current}, peak ${peak}`}
					className="w-full rounded-xl border border-border/70 bg-card/80"
					style={{ height: CHART_HEIGHT }}
				/>
			)}
		</div>
	);
}
