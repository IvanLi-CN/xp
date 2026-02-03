import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

const meta = {
	title: "Pages/UsersPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/users",
		},
		mockApi: {
			data: {
				nodeQuotas: [
					{
						user_id: "user-1",
						node_id: "node-1",
						quota_limit_bytes: 10 * 2 ** 30,
						quota_reset_source: "user",
					},
					{
						user_id: "user-1",
						node_id: "node-2",
						quota_limit_bytes: 5 * 2 ** 30,
						quota_reset_source: "user",
					},
					{
						user_id: "user-2",
						node_id: "node-2",
						quota_limit_bytes: 5 * 2 ** 30,
						quota_reset_source: "user",
					},
				],
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
			await canvas.findByRole("heading", { name: "Users" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText("Quota usage (remaining/limit)"),
		).toBeInTheDocument();

		// Aggregated quota for user-1: 10 GiB + 5 GiB.
		await expect(await canvas.findByText("15 GiB/15 GiB")).toBeInTheDocument();
	},
};
