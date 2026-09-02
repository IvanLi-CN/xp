import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	RESOURCE_HISTORY_CHARTS,
	RUNTIME_RESOURCE_HISTORY_CHARTS,
	buildResourceHistoryChartOption,
} from "./ResourceSnapshotPanel";
import type { EChartsThemePalette } from "./echarts-theme";

const resourceHistoryBase = Date.parse(
	fixtureCatalog.timestamp.t20260901T000000(),
);

function catalogTimestampOffset(index: number): string {
	return new Date(resourceHistoryBase + index * 15_000).toISOString();
}

const palette: EChartsThemePalette = {
	axis: "axis",
	axisPointer: "axisPointer",
	currentArea: "currentArea",
	currentDay: "currentDay",
	grid: "grid",
	primary: "primary",
	reference: "reference",
	tooltip: {
		background: "tooltipBackground",
		border: "tooltipBorder",
		foreground: "tooltipForeground",
		muted: "tooltipMuted",
		shadow: "tooltipShadow",
		shadowBlur: 12,
		shadowOffsetX: 0,
		shadowOffsetY: 4,
	},
};

describe("buildResourceHistoryChartOption", () => {
	it("renders each fixed resource series with the TCP history chart contract", () => {
		const points = [
			{
				observed_at: catalogTimestampOffset(0),
				value: fixtureCatalog.number.value20(),
			},
			{
				observed_at: catalogTimestampOffset(1),
				value: null,
			},
			{
				observed_at: catalogTimestampOffset(2),
				value: fixtureCatalog.number.value50(),
			},
		];

		for (const chart of RESOURCE_HISTORY_CHARTS) {
			const option = buildResourceHistoryChartOption(
				Object.fromEntries([[chart.series[0].metric, points]]),
				palette,
				chart,
			);

			expect(option).toMatchObject({
				grid: { top: 24, right: 18, bottom: 36, left: 42 },
				xAxis: {
					type: "time",
					min: 1_788_220_800_000,
					max: 1_788_220_830_000,
				},
				yAxis: {
					type: "value",
					min: 0,
					axisLabel: { formatter: expect.any(Function) },
				},
				series: [
					{
						name: chart.series[0].name,
						type: "line",
						data: [
							[1_788_220_800_000, 20],
							[1_788_220_815_000, null],
							[1_788_220_830_000, 50],
						],
						step: "end",
						smooth: false,
						connectNulls: false,
						showSymbol: false,
						lineStyle: { width: 2, color: "primary", join: "round" },
						areaStyle: { color: "primary", opacity: 0.18 },
					},
				],
			});

			if (chart.unit === "percent") {
				expect(option.yAxis).toMatchObject({ max: 100 });
			} else {
				expect(option.yAxis).not.toMatchObject({ max: 100 });
			}
		}
	});
});

describe("runtime resource charts", () => {
	it("keeps the runtime detail metrics fixed while grouping related series", () => {
		expect(
			RUNTIME_RESOURCE_HISTORY_CHARTS.map((chart) =>
				chart.series.map((series) => series.metric),
			),
		).toEqual([
			["cpu_percent"],
			["rss_bytes", "pss_bytes"],
			["read_bytes_per_second", "write_bytes_per_second"],
			["fd_count"],
			["thread_count"],
		]);
	});

	it("renders paired runtime metrics with the existing blue comparison-series style", () => {
		const chart = RUNTIME_RESOURCE_HISTORY_CHARTS.find(
			(candidate) => candidate.key === "memory",
		);
		expect(chart).toBeDefined();
		if (!chart) return;

		const option = buildResourceHistoryChartOption(
			{
				rss_bytes: [
					{
						observed_at: fixtureCatalog.timestamp.t20260901T000000(),
						value: fixtureCatalog.number.value100(),
					},
				],
				pss_bytes: [
					{
						observed_at: fixtureCatalog.timestamp.t20260901T000000(),
						value: fixtureCatalog.number.value60(),
					},
				],
			},
			palette,
			chart,
		);

		expect(option.series).toMatchObject([
			{
				name: "RSS",
				lineStyle: { width: 2, color: "primary", join: "round" },
				areaStyle: { color: "primary", opacity: 0.18 },
			},
			{
				name: "PSS",
				lineStyle: { width: 2, color: "#3478c6", join: "round" },
			},
		]);
	});
});
