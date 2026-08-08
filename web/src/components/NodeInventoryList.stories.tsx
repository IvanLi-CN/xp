import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type { AdminNodeRuntimeListItem } from "../api/adminNodeRuntime";
import {
	LIST_LAYOUT_BREAKPOINT_PX,
	NodeInventoryList,
} from "./NodeInventoryList";

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
		node_id: fixtureCatalog.slotString.s229(),
		node_name: fixtureCatalog.slotString.s280(),
		api_base_url: fixtureCatalog.slotString.s281(),
		access_host: fixtureCatalog.slotString.s282(),
		summary: {
			status: "up",
			updated_at: fixtureCatalog.slotString.s283(),
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
		node_id: fixtureCatalog.slotString.s233(),
		node_name: fixtureCatalog.slotString.s99(),
		api_base_url: fixtureCatalog.slotString.s235(),
		access_host: fixtureCatalog.slotString.s284(),
		summary: {
			status: "degraded",
			updated_at: fixtureCatalog.slotString.s283(),
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

export const DesktopTable: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("columnheader", { name: "Actions" }),
		).toBeInTheDocument();
		await expect(canvas.getAllByRole("link", { name: "Details" })).toHaveLength(
			2,
		);
		await expect(
			canvas.getAllByRole("link", { name: "Open on node" }),
		).toHaveLength(2);
	},
};

export const MobileCards: Story = {
	decorators: [
		(Story) => (
			<div
				className="max-w-full"
				style={{ width: `${LIST_LAYOUT_BREAKPOINT_PX - 1}px` }}
			>
				<Story />
			</div>
		),
	],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.queryByRole("table")).toBeNull();
		await expect(canvas.getAllByRole("link", { name: "Details" })).toHaveLength(
			2,
		);
		await expect(
			canvas.getAllByRole("link", { name: "Open on node" }),
		).toHaveLength(2);
	},
};
