import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

import type { AdminNode } from "../api/adminNodes";
import { buildDenseNodeIpUsageStories } from "../storybook/ipUsageStoryData";

const node: AdminNode = {
	node_id: "01J000000000000000000000001",
	node_name: "node-a",
	access_host: "node-a.example.invalid",
	api_base_url: "https://node-a.example.invalid",
	quota_limit_bytes: 0,
	quota_reset: {
		policy: "monthly",
		day_of_month: 1,
		tz_offset_minutes: null,
	},
};

const ipUsageReports = buildDenseNodeIpUsageStories(node);

const meta = {
	title: "Pages/NodeDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: `/nodes/${node.node_id}`,
		},
		mockApi: {
			data: {
				nodes: [node],
				nodeIpUsageByNodeId: {
					[node.node_id]: ipUsageReports,
				},
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("heading", { name: /service runtime/i }),
		).toBeInTheDocument();
		await expect(await canvas.findByText(/^key events$/i)).toBeInTheDocument();
	},
};

export const IpUsageTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByRole("tab", { name: "IP usage" }));
		await expect(
			await canvas.findByRole("button", { name: "198.51.100.88" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", { name: "203.0.113.55" }),
		).toBeInTheDocument();
	},
};

export const IpUsageTab7d: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByRole("tab", { name: "IP usage" }));
		await userEvent.click(await canvas.findByRole("button", { name: "7d" }));
		await expect(
			await canvas.findByRole("button", { name: "7d" }),
		).toHaveAttribute("aria-pressed", "true");
		await expect(
			await canvas.findByRole("button", { name: "198.51.100.88" }),
		).toBeInTheDocument();
	},
};
