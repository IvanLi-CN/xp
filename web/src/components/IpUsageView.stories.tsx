import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import { buildDenseNodeIpUsageStories } from "../storybook/ipUsageStoryData";
import { IpUsageView } from "./IpUsageView";

const reports = buildDenseNodeIpUsageStories();

const meta = {
	title: "Components/IpUsageView",
	component: IpUsageView,
	tags: ["autodocs", "coverage-ui"],
	args: {
		onWindowChange: () => {},
		title: "IP usage",
		description:
			"Per-minute unique inbound IP counts, occupancy lanes, and aggregated IP rows.",
		window: "24h",
		report: reports["24h"],
	},
	render: function Render(args) {
		const [window, setWindow] = useState(args.window);
		return <IpUsageView {...args} window={window} onWindowChange={setWindow} />;
	},
} satisfies Meta<typeof IpUsageView>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	args: {
		report: reports["24h"],
		window: "24h",
	},
};

export const Dense7d: Story = {
	args: {
		report: reports["7d"],
		window: "7d",
	},
};

export const LoadingLatest: Story = {
	args: { isWindowPending: true },
};

export const Empty: Story = {
	args: {
		emptyTitle: "No inbound IP activity",
		report: {
			...reports["24h"],
			warnings: [],
			unique_ip_series: reports["24h"].unique_ip_series.map((point) => ({
				...point,
				count: 0,
			})),
			timeline: [],
			ips: [],
		},
	},
};

export const OnlineStatsUnavailable: Story = {
	args: {
		report: {
			...reports["24h"],
			warnings: [
				{
					code: "online_stats_unavailable",
					message:
						"Xray online IP stats are unavailable; enable statsUserOnline to collect inbound IP usage.",
				},
			],
			unique_ip_series: [],
			timeline: [],
			ips: [],
		},
	},
};
