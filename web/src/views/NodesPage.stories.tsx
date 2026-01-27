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
						node_id: "01KF40H0JW519AM6JNZFQKXXE1",
						node_name: "hinet",
						access_host: "hinet.node.example.invalid",
						api_base_url: "https://hinet-api.example.invalid",
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
					{
						node_id: "01KFTEA58X1RXXVDRD6EPFB63Y",
						node_name: "hkl",
						access_host: "hkl.node.example.invalid",
						api_base_url: "https://hkl-api.example.invalid",
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
