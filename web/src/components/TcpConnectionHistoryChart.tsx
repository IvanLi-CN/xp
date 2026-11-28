import type { EChartsOption } from "echarts";
import ReactEChartsCore from "echarts-for-react/lib/core";
import { LineChart } from "echarts/charts";
import {
	GridComponent,
	LegendComponent,
	TooltipComponent,
} from "echarts/components";
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

echarts.use([
	GridComponent,
	LegendComponent,
	TooltipComponent,
	LineChart,
	SVGRenderer,
]);

const SVG_RENDERER = { renderer: "svg" } as const;
const CHART_HEIGHT = 260;

const CLASS_LABELS: Record<string, string> = {
	inbound: "Inbound",
	control_plane: "Control plane",
	outbound_runtime: "Runtime outbound",
	outbound_other: "Other outbound",
};

// Categorical palette: hues chosen far apart so stacked bands stay distinguishable in both
// themes. Total is a decoupled envelope, so it keeps the primary accent.
const CLASS_PALETTE: Record<string, { light: string; dark: string }> = {
	Total: { light: "rgb(6, 182, 212)", dark: "rgb(34, 211, 238)" },
	inbound: { light: "rgb(37, 99, 235)", dark: "rgb(96, 165, 250)" },
	"Control plane": { light: "rgb(217, 119, 6)", dark: "rgb(251, 191, 36)" },
	"Runtime outbound": { light: "rgb(22, 163, 74)", dark: "rgb(74, 222, 128)" },
	"Other outbound": { light: "rgb(147, 51, 234)", dark: "rgb(192, 132, 252)" },
};

type Series = {
	name: string;
	points: Array<[number, number]>;
};

function timeLabel(value: number): string {
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return "";
	return date
		.toLocaleString("en-GB", {
			hour: "2-digit",
			minute: "2-digit",
			hour12: false,
			timeZone: "UTC",
		})
		.replace(/^24/, "00");
}

function isDarkTheme(): boolean {
	if (typeof document === "undefined") return false;
	return document.documentElement.classList.contains("dark");
}

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
		const dark = isDarkTheme();
		const colorFor = (name: string) => {
			const entry = CLASS_PALETTE[name];
			return entry ? (dark ? entry.dark : entry.light) : palette.reference;
		};
		const colors = series.map((item) => colorFor(item.name));
		return {
			animationDuration: 220,
			color: colors,
			grid: { top: 56, right: 18, bottom: 36, left: 42 },
			legend: {
				top: 0,
				left: 0,
				itemGap: 14,
				itemWidth: 10,
				itemHeight: 10,
				icon: "roundRect",
				textStyle: { color: palette.axis, fontSize: 12 },
				data: series.map((item) => item.name),
			},
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
				axisLabel: {
					color: palette.axis,
					hideOverlap: true,
					formatter: timeLabel,
				},
				min: new Date(report.window_start ?? "").getTime() || undefined,
				max: new Date(report.window_end ?? "").getTime() || undefined,
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
				// Total is the enveloping series, so it must not participate in the stack.
				stack: item.name === "Total" ? undefined : "classes",
				z: item.name === "Total" ? 3 : 1,
				step: "end" as const,
				smooth: false,
				showSymbol: false,
				emphasis: STATIC_LINE_SERIES_EMPHASIS,
				data: item.points,
				lineStyle:
					item.name === "Total" ? { width: 2.5 } : { width: 1.5, opacity: 0.9 },
				areaStyle:
					item.name === "Total" ? { opacity: 0.06 } : { opacity: 0.35 },
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
