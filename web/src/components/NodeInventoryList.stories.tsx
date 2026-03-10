import type { Meta, StoryObj } from "@storybook/react";
import { fn } from "@storybook/test";

import type { AdminNodeRuntimeListItem } from "../api/adminNodeRuntime";
import { NodeInventoryList } from "./NodeInventoryList";

function buildRecentSlots(
	startIso: string,
	statuses: Array<"up" | "degraded" | "down" | "unknown">,
) {
	const start = Date.parse(startIso);
	return statuses.map((status, index) => ({
		slot_start: new Date(start + index * 30 * 60 * 1000).toISOString(),
		status,
	}));
}

const sampleNodes: AdminNodeRuntimeListItem[] = [
	{
		node_id: "01J000000000000000000000001",
		node_name: "tokyo-edge-a",
		api_base_url:
			"https://tokyo-edge-a.example.invalid/admin/runtime/super/long/path",
		access_host: "tokyo-edge-a.example.invalid",
		summary: {
			status: "up",
			updated_at: "2026-03-03T12:00:00Z",
		},
		components: [
			{
				component: "xp",
				status: "up",
				consecutive_failures: 0,
				recoveries_observed: 4,
				restart_attempts: 0,
			},
			{
				component: "xray",
				status: "up",
				consecutive_failures: 0,
				recoveries_observed: 4,
				restart_attempts: 0,
			},
		],
		recent_slots: buildRecentSlots("2026-03-03T00:00:00Z", [
			"up",
			"up",
			"degraded",
			"up",
			"up",
			"up",
			"up",
			"up",
			"up",
			"up",
			"up",
			"up",
		]),
	},
	{
		node_id: "01J000000000000000000000002",
		node_name: "",
		api_base_url: "https://node-b.example.invalid",
		access_host: "very-long-hostname-node-b.example.invalid",
		summary: {
			status: "degraded",
			updated_at: "2026-03-03T12:00:00Z",
		},
		components: [
			{
				component: "xp",
				status: "down",
				consecutive_failures: 2,
				recoveries_observed: 0,
				restart_attempts: 2,
			},
			{
				component: "xray",
				status: "unknown",
				consecutive_failures: 1,
				recoveries_observed: 0,
				restart_attempts: 1,
			},
			{
				component: "cloudflared",
				status: "down",
				consecutive_failures: 3,
				recoveries_observed: 0,
				restart_attempts: 2,
			},
		],
		recent_slots: buildRecentSlots("2026-03-03T00:00:00Z", [
			"up",
			"up",
			"down",
			"up",
			"unknown",
			"up",
			"degraded",
			"up",
			"up",
			"down",
			"up",
			"up",
		]),
	},
];

const meta: Meta<typeof NodeInventoryList> = {
	title: "Components/NodeInventoryList",
	component: NodeInventoryList,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "padded",
	},
	args: {
		items: sampleNodes,
		partial: false,
		unreachableNodes: [],
		isRefreshing: false,
		onRefresh: fn(),
	},
};

export default meta;

type Story = StoryObj<typeof NodeInventoryList>;

export const Default: Story = {};

export const PartialResult: Story = {
	args: {
		partial: true,
		unreachableNodes: ["01J0000000000000000000000ff"],
	},
};

export const MobileCards: Story = {
	decorators: [
		(Story) => (
			<div className="w-[390px]">
				<Story />
			</div>
		),
	],
};
