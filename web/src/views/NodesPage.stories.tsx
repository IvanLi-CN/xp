import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const meta = {
	title: "Pages/NodesPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/nodes",
		},
		mockApi: {
			data: {
				nodes: [
					{
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
					},
					{
						node_id: "01J000000000000000000000002",
						node_name: "node-b",
						access_host: "node-b.example.invalid",
						api_base_url: "https://node-b.example.invalid",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 15,
							tz_offset_minutes: null,
						},
					},
				],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithJoinToken: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findAllByRole("link", { name: "Details" }),
		).toHaveLength(2);
		const openLinks = await canvas.findAllByRole("link", {
			name: "Open on node",
		});
		await expect(openLinks).toHaveLength(2);
		await expect(openLinks[0]).toHaveAttribute(
			"href",
			"https://node-a.example.invalid/iframe.html?id=pages-nodespage--with-join-token&viewMode=%7B%7BviewMode%7D%7D&login_token=storybook-admin-token",
		);
		await userEvent.click(
			canvas.getByRole("button", { name: /create token/i }),
		);
		await expect(
			await canvas.findByText(/xp-ops deploy command/i),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/sudo xp-ops deploy/i),
		).toBeInTheDocument();
	},
};
