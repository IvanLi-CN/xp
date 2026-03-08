import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import type { AdminNodeIpUsageResponse } from "../api/adminIpUsage";
import { IpUsageView } from "./IpUsageView";

const baseReport: Pick<
	AdminNodeIpUsageResponse,
	| "window_start"
	| "window_end"
	| "warnings"
	| "unique_ip_series"
	| "timeline"
	| "ips"
> = {
	window_start: "2026-03-08T00:00:00Z",
	window_end: "2026-03-08T00:02:00Z",
	warnings: [],
	unique_ip_series: [
		{ minute: "2026-03-08T00:00:00Z", count: 1 },
		{ minute: "2026-03-08T00:01:00Z", count: 2 },
		{ minute: "2026-03-08T00:02:00Z", count: 1 },
	],
	timeline: [
		{
			lane_key: "edge-tokyo::203.0.113.7",
			endpoint_id: "endpoint-1",
			endpoint_tag: "edge-tokyo",
			ip: "203.0.113.7",
			minutes: 2,
			segments: [
				{
					start_minute: "2026-03-08T00:00:00Z",
					end_minute: "2026-03-08T00:01:00Z",
				},
			],
		},
		{
			lane_key: "edge-osaka::203.0.113.9",
			endpoint_id: "endpoint-2",
			endpoint_tag: "edge-osaka",
			ip: "203.0.113.9",
			minutes: 1,
			segments: [
				{
					start_minute: "2026-03-08T00:02:00Z",
					end_minute: "2026-03-08T00:02:00Z",
				},
			],
		},
	],
	ips: [
		{
			ip: "203.0.113.7",
			minutes: 2,
			endpoint_tags: ["edge-tokyo"],
			region: "Japan / Tokyo",
			operator: "ExampleNet",
			last_seen_at: "2026-03-08T00:01:00Z",
		},
		{
			ip: "203.0.113.9",
			minutes: 1,
			endpoint_tags: ["edge-osaka"],
			region: "Japan / Osaka",
			operator: "CarrierNet",
			last_seen_at: "2026-03-08T00:02:00Z",
		},
	],
};

const meta = {
	title: "Components/IpUsageView",
	component: IpUsageView,
	args: {
		onWindowChange: () => {},
		title: "IP usage",
		description:
			"Per-minute unique inbound IP counts, occupancy lanes, and aggregated IP rows.",
		window: "24h",
		report: baseReport,
	},
	render: function Render(args) {
		const [window, setWindow] = useState(args.window);
		return <IpUsageView {...args} window={window} onWindowChange={setWindow} />;
	},
} satisfies Meta<typeof IpUsageView>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	args: {},
};

export const Empty: Story = {
	args: {
		emptyTitle: "No inbound IP activity",
		report: {
			...baseReport,
			unique_ip_series: [],
			timeline: [],
			ips: [],
		},
	},
};

export const OnlineStatsUnavailable: Story = {
	args: {
		report: {
			...baseReport,
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
