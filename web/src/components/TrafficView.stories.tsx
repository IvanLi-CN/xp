import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import { useState } from "react";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type {
	TrafficReport,
	TrafficSeriesPoint,
	TrafficWindow,
} from "../api/adminTraffic";
import { TrafficView } from "./TrafficView";

function makePoint(currentDay: boolean, gap = false): TrafficSeriesPoint {
	if (gap) {
		return {
			start_at: fixtureCatalog.timestamp.baseline(),
			end_at: fixtureCatalog.timestamp.recent(),
			uplink_bytes: null,
			downlink_bytes: null,
			total_bytes: null,
			complete: false,
			is_current_day: currentDay,
		};
	}

	return {
		start_at: fixtureCatalog.timestamp.baseline(),
		end_at: fixtureCatalog.timestamp.recent(),
		uplink_bytes: fixtureCatalog.slotNumber.n2(),
		downlink_bytes: fixtureCatalog.slotNumber.n3(),
		total_bytes: fixtureCatalog.slotNumber.n6(),
		complete: true,
		is_current_day: currentDay,
	};
}

function makeReport(window: TrafficWindow, gap = false): TrafficReport {
	const count = window === "24h" ? 288 : 31;
	const summary =
		window === "24h"
			? {
					uplink_bytes: fixtureCatalog.slotNumber.n32(),
					downlink_bytes: fixtureCatalog.slotNumber.n33(),
					total_bytes: fixtureCatalog.slotNumber.n34(),
				}
			: {
					uplink_bytes: fixtureCatalog.slotNumber.n35(),
					downlink_bytes: fixtureCatalog.slotNumber.n36(),
					total_bytes: fixtureCatalog.slotNumber.n37(),
				};
	const current = Array.from({ length: count }, (_, index) =>
		makePoint(
			window === "31d" && index === count - 1,
			gap && index > 112 && index < 124,
		),
	);
	const reference = current.map(() => makePoint(false));
	return {
		window,
		window_start_at: fixtureCatalog.timestamp.baseline(),
		window_end_at: fixtureCatalog.timestamp.recent(),
		timezone: "UTC",
		summary: {
			mode: "cycle",
			cycle_start_at: fixtureCatalog.timestamp.earlier(),
			cycle_end_at: fixtureCatalog.timestamp.later(),
			...summary,
			complete: !gap,
			tracking_since: fixtureCatalog.timestamp.baseline(),
		},
		current,
		reference,
		partial: gap,
		last_sample_at: fixtureCatalog.slotString.s177(),
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

export const LoadingLatest: Story = {
	args: { isWindowPending: true },
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
