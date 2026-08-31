import type { Meta, StoryObj } from "@storybook/react";

import type { ServiceMonitorHistoryResponse } from "../api/adminServiceMonitors";
import { ServiceMonitorHistoryChart } from "./ServiceMonitorHistoryChart";

function history(points = 18): ServiceMonitorHistoryResponse {
	return {
		monitor_id: "01JMONITOR00000000000000001",
		resolution: "1m",
		points: Array.from({ length: points }, (_, index) => ({
			start_unix_seconds: 1_785_192_000 + index * 300,
			end_unix_seconds: 1_785_192_299 + index * 300,
			rollup: {
				expected: 5,
				executed: 5,
				successes: index === 9 ? 3 : 5,
				failures: index === 9 ? 2 : 0,
				unsupported: 0,
				suspended: 0,
				latency_count: 5,
				latency_sum_ms: 190 + index * 3,
				latency_min_ms: 25,
				latency_max_ms: 68,
				latency_histogram: {
					underflow: 0,
					buckets: Array.from({ length: 32 }, (_, bucket) =>
						bucket === 5 ? 5 : 0,
					),
					overflow: 0,
				},
				errors:
					index === 9 ? { connect_timeout: 2 } : ({} as Record<string, number>),
			},
			availability_percent: index === 9 ? 60 : 100,
			coverage_percent: 100,
		})),
		truncated: false,
		quality: "complete",
		coverage_percent: 100,
		watermark_unix_seconds: 1_785_278_382,
		gaps: [],
		skew_seconds: 0,
		freshness_seconds: 18,
	};
}

const meta = {
	title: "Components/ServiceMonitorHistoryChart",
	component: ServiceMonitorHistoryChart,
	tags: ["autodocs", "coverage-ui"],
	args: { history: history() },
	decorators: [
		(Story) => (
			<div className="min-h-[22rem] bg-background p-6">
				<Story />
			</div>
		),
	],
} satisfies Meta<typeof ServiceMonitorHistoryChart>;

export default meta;
type Story = StoryObj<typeof meta>;

export const TwentyFourHourTrend: Story = {};
export const EmptyHistory: Story = { args: { history: history(0) } };
