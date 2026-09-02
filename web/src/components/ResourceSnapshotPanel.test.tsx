import { describe, expect, it } from "vitest";

import { buildResourceCpuHistoryChartOption } from "./ResourceSnapshotPanel";
import type { EChartsThemePalette } from "./echarts-theme";

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

describe("buildResourceCpuHistoryChartOption", () => {
	it("renders CPU samples as a time series and retains sampling gaps", () => {
		const option = buildResourceCpuHistoryChartOption(
			[
				{ observed_at: "2026-09-01T10:00:00Z", value: 18.4 },
				{ observed_at: "2026-09-01T10:00:15Z", value: null },
				{ observed_at: "2026-09-01T10:00:30Z", value: 52.1 },
			],
			palette,
		);

		expect(option).toMatchObject({
			xAxis: {
				type: "category",
				boundaryGap: false,
				data: [
					"2026-09-01T10:00:00Z",
					"2026-09-01T10:00:15Z",
					"2026-09-01T10:00:30Z",
				],
			},
			yAxis: { type: "value", min: 0, max: 100 },
			series: [
				{
					name: "CPU busy",
					type: "line",
					data: [18.4, null, 52.1],
					smooth: false,
					connectNulls: false,
					symbol: "none",
					lineStyle: { color: "primary", width: 2, join: "round" },
				},
			],
		});
	});
});
