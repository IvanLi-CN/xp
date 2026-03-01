import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const USER_ID_1 = "01HF7YAT00T6RTJH6T9Z8ZPMDV";
const USER_ID_2 = "01HF7YAT01YVKWQ847J5T9EY84";

const meta = {
	title: "Pages/UserDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: `/users/${USER_ID_1}`,
		},
		mockApi: {
			data: {
				userAccessByUserId: {
					[USER_ID_1]: [
						{
							user_id: USER_ID_1,
							endpoint_id: "endpoint-1",
							node_id: "node-1",
						},
						{
							user_id: USER_ID_1,
							endpoint_id: "endpoint-2",
							node_id: "node-2",
						},
					],
				},
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
						quota_reset_source: "node",
					},
				],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const User1: Story = {};

export const User2: Story = {
	parameters: {
		router: {
			initialEntry: `/users/${USER_ID_2}`,
		},
	},
};

export const AccessTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Access" }),
		);
		await expect(
			await canvas.findByText("Selected endpoints: 2"),
		).toBeInTheDocument();
	},
};

export const QuotaStatusTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const tab = await canvas.findByRole("button", { name: "Quota status" });
		await userEvent.click(tab);
		await expect(tab).toHaveClass("tab-active");
	},
};
