import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

const USER_ID_1 = "01HF7YAT00T6RTJH6T9Z8ZPMDV";
const USER_ID_2 = "01HF7YAT01YVKWQ847J5T9EY84";

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
						user_id: USER_ID_1,
						node_id: "node-1",
						quota_limit_bytes: 10 * 2 ** 30,
						quota_reset_source: "user",
					},
					{
						user_id: USER_ID_1,
						node_id: "node-2",
						quota_limit_bytes: 5 * 2 ** 30,
						quota_reset_source: "user",
					},
					{
						user_id: USER_ID_2,
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
		await expect(await canvas.findByText("Quota usage")).toBeInTheDocument();
		await expect(
			await canvas.findByText("remaining/limit"),
		).toBeInTheDocument();

		// Aggregated quota for the first user: 10 GiB + 5 GiB.
		await expect(await canvas.findByText("15 GiB/15 GiB")).toBeInTheDocument();
	},
};
