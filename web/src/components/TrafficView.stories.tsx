import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import { useState } from "react";

import type {
	TrafficReport,
	TrafficSeriesPoint,
	TrafficWindow,
} from "../api/adminTraffic";
import { TrafficView } from "./TrafficView";

function makePoint(
	start: Date,
	amount: number | null,
	currentDay: boolean,
): TrafficSeriesPoint {
	const end = new Date(start);
	end.setUTCMinutes(end.getUTCMinutes() + 5);
	return {
		start_at: start.toISOString(),
		end_at: end.toISOString(),
		uplink_bytes: amount == null ? null : Math.round(amount * 0.42),
		downlink_bytes: amount == null ? null : Math.round(amount * 0.58),
		total_bytes: amount,
		complete: amount != null,
		is_current_day: currentDay,
	};
}

function makeReport(window: TrafficWindow, gap = false): TrafficReport {
	const count = window === "24h" ? 288 : 31;
	const end = new Date("2026-07-28T12:00:00.000Z");
	const stepMs = window === "24h" ? 5 * 60_000 : 24 * 60 * 60_000;
	const start = new Date(end.getTime() - (count - 1) * stepMs);
	const current = Array.from({ length: count }, (_, index) => {
		const at = new Date(start.getTime() + index * stepMs);
		const amount =
			gap && index > 112 && index < 124
				? null
				: 120_000_000 + ((index * 19) % 7) * 11_000_000;
		return makePoint(at, amount, window === "31d" && index === count - 1);
	});
	const reference = current.map((point, index) =>
		makePoint(
			new Date(new Date(point.start_at).getTime() - count * stepMs),
			95_000_000 + ((index * 13) % 5) * 8_000_000,
			false,
		),
	);
	return {
		window,
		window_start_at: current[0]?.start_at ?? end.toISOString(),
		window_end_at: current.at(-1)?.end_at ?? end.toISOString(),
		timezone: "UTC",
		summary: {
			mode: "cycle",
			cycle_start_at: "2026-07-01T00:00:00Z",
			cycle_end_at: "2026-08-01T00:00:00Z",
			uplink_bytes: 2_400_000_000,
			downlink_bytes: 3_300_000_000,
			total_bytes: 5_700_000_000,
			complete: !gap,
			tracking_since: "2026-07-01T00:00:00Z",
		},
		current,
		reference,
		partial: gap,
		last_sample_at: "2026-07-28T12:00:00Z",
		warnings: gap ? ["sampling gap in current window"] : [],
	};
}

const meta = {
	title: "Components/TrafficView",
	component: TrafficView,
	tags: ["autodocs", "coverage-ui"],
	args: {
		window: "24h" as const,
		report: makeReport("24h"),
		onWindowChange: () => {},
	},
	render: function Render(args) {
		const [window, setWindow] = useState(args.window);
		return (
			<TrafficView
				{...args}
				window={window}
				report={window === "24h" ? makeReport("24h") : makeReport("31d")}
				onWindowChange={setWindow}
			/>
		);
	},
} satisfies Meta<typeof TrafficView>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Last24Hours: Story = {};

export const Last31Days: Story = {
	args: { window: "31d", report: makeReport("31d") },
};

export const SamplingGap: Story = {
	args: { report: makeReport("24h", true) },
};

export const TooltipPreview: Story = {
	args: { tooltipPreviewIndex: 144 },
	play: async ({ canvasElement }) => {
		await expect(
			within(canvasElement).getByTestId("traffic-tooltip-preview"),
		).toBeVisible();
	},
};

export const Empty: Story = {
	args: {
		report: {
			...makeReport("24h"),
			current: makeReport("24h").current.map((point) => ({
				...point,
				uplink_bytes: null,
				downlink_bytes: null,
				total_bytes: null,
				complete: false,
			})),
			partial: true,
		},
	},
};
