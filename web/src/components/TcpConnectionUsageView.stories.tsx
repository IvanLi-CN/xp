import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import type { AdminNode } from "../api/adminNodes";
import { buildDenseNodeTcpConnectionStories } from "../storybook/tcpConnectionStoryData";
import { TcpConnectionUsageView } from "./TcpConnectionUsageView";

const demoNode: AdminNode = {
	node_id: "demo-node-1",
	node_name: "tokyo-1",
	access_host: "tokyo-1.example.com",
	api_base_url: "https://tokyo-1.example.com",
	quota_limit_bytes: 0,
	quota_reset: {
		policy: "monthly",
		day_of_month: 1,
		tz_offset_minutes: null,
	},
};

const reports = buildDenseNodeTcpConnectionStories(demoNode);
const tooltipPreviewMinute = "2026-03-07T01:30:00.000Z";

const meta = {
	title: "Components/TcpConnectionUsageView",
	component: TcpConnectionUsageView,
	tags: ["autodocs", "coverage-ui"],
	args: {
		window: "24h",
		onWindowChange: () => {},
		report: reports["24h"],
	},
	render: function Render(args) {
		const [window, setWindow] = useState(args.window);
		return (
			<TcpConnectionUsageView
				{...args}
				window={window}
				onWindowChange={setWindow}
			/>
		);
	},
} satisfies Meta<typeof TcpConnectionUsageView>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	args: {
		window: "24h",
		report: reports["24h"],
	},
};

export const Dense7d: Story = {
	args: {
		window: "7d",
		report: reports["7d"],
	},
};

export const TooltipBreakdownPreview: Story = {
	args: {
		window: "24h",
		report: reports["24h"],
		tooltipPreviewMinute,
	},
};

export const UnsupportedPlatform: Story = {
	args: {
		window: "24h",
		report: {
			...reports["24h"],
			warnings: [
				{
					code: "unsupported_platform",
					message:
						"TCP connection count history is currently only supported on Linux nodes.",
				},
			],
		},
	},
};

export const EmptySelection: Story = {
	args: {
		window: "24h",
		report: {
			...reports["24h"],
			per_endpoint_series: reports["24h"].per_endpoint_series.map((series) => ({
				...series,
				series: series.series.map((point) => ({
					...point,
					count: 0,
				})),
			})),
		},
	},
};
