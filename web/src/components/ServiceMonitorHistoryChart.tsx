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

import type { ServiceMonitorHistoryResponse } from "../api/adminServiceMonitors";
import { useEChartsThemePalette } from "./echarts-theme";

echarts.use([
	GridComponent,
	LegendComponent,
	TooltipComponent,
	LineChart,
	SVGRenderer,
]);

type ServiceMonitorHistoryChartProps = {
	history: ServiceMonitorHistoryResponse;
	height?: number;
};

function timestamp(unixSeconds: number): string {
	return new Date(unixSeconds * 1000)
		.toISOString()
		.replace("T", " ")
		.slice(0, 16);
}

function averageLatency(
	point: ServiceMonitorHistoryResponse["points"][number],
) {
	const { latency_count: count, latency_sum_ms: total } = point.rollup;
	return count > 0 ? Math.round(total / count) : null;
}

export function ServiceMonitorHistoryChart({
	history,
	height = 280,
}: ServiceMonitorHistoryChartProps) {
	const palette = useEChartsThemePalette();
	const option = useMemo<EChartsOption>(() => {
		const labels = history.points.map((point) =>
			timestamp(point.start_unix_seconds),
		);
		return {
			animation: false,
			color: ["#0f9f6e", "#d39a17", "#3478c6"],
			grid: { top: 24, right: 48, bottom: 44, left: 44 },
			tooltip: { trigger: "axis", confine: true },
			legend: {
				bottom: 4,
				textStyle: { color: palette.axis },
			},
			xAxis: {
				type: "category",
				data: labels,
				axisLabel: { color: palette.axis, hideOverlap: true },
				axisLine: { lineStyle: { color: palette.grid } },
			},
			yAxis: [
				{
					type: "value",
					min: 0,
					max: 100,
					axisLabel: { color: palette.axis, formatter: "{value}%" },
					splitLine: { lineStyle: { color: palette.grid } },
				},
				{
					type: "value",
					axisLabel: { color: palette.axis, formatter: "{value} ms" },
					splitLine: { show: false },
				},
			],
			series: [
				{
					name: "Availability",
					type: "line",
					smooth: false,
					connectNulls: false,
					data: history.points.map((point) => point.availability_percent),
				},
				{
					name: "Coverage",
					type: "line",
					smooth: false,
					connectNulls: false,
					data: history.points.map((point) => point.coverage_percent),
				},
				{
					name: "Average latency",
					type: "line",
					yAxisIndex: 1,
					smooth: false,
					connectNulls: false,
					data: history.points.map(averageLatency),
				},
			],
		};
	}, [history.points, palette.axis, palette.grid]);

	if (history.points.length === 0) {
		return (
			<p className="py-10 text-center text-sm text-muted-foreground">
				No scheduled checks in this range.
			</p>
		);
	}
	return (
		<div aria-label="Service monitor history chart" className="w-full">
			<ReactEChartsCore
				echarts={echarts}
				option={option}
				notMerge
				lazyUpdate
				style={{ height, width: "100%" }}
				opts={{ renderer: "svg" }}
			/>
		</div>
	);
}
