import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type { AdminNodeRuntimeListItem } from "../api/adminNodeRuntime";
import {
	LIST_LAYOUT_BREAKPOINT_PX,
	NodeInventoryList,
} from "./NodeInventoryList";

type RecentSlotStatus = "up" | "degraded" | "down" | "unknown";

type RecentSlotStatuses = readonly [
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
	RecentSlotStatus,
];

function buildRecentSlots(statuses: RecentSlotStatuses) {
	const [
		slot0,
		slot1,
		slot2,
		slot3,
		slot4,
		slot5,
		slot6,
		slot7,
		slot8,
		slot9,
		slot10,
		slot11,
	] = statuses;

	return [
		{ slot_start: fixtureCatalog.slotString.s4(), status: slot0 },
		{ slot_start: fixtureCatalog.slotString.s5(), status: slot1 },
		{ slot_start: fixtureCatalog.slotString.s6(), status: slot2 },
		{ slot_start: fixtureCatalog.slotString.s7(), status: slot3 },
		{ slot_start: fixtureCatalog.slotString.s8(), status: slot4 },
		{ slot_start: fixtureCatalog.slotString.s9(), status: slot5 },
		{ slot_start: fixtureCatalog.slotString.s10(), status: slot6 },
		{ slot_start: fixtureCatalog.slotString.s11(), status: slot7 },
		{ slot_start: fixtureCatalog.slotString.s12(), status: slot8 },
		{ slot_start: fixtureCatalog.slotString.s13(), status: slot9 },
		{ slot_start: fixtureCatalog.slotString.s14(), status: slot10 },
		{ slot_start: fixtureCatalog.slotString.s15(), status: slot11 },
	];
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
		recent_slots: buildRecentSlots([
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
		recent_slots: buildRecentSlots([
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
