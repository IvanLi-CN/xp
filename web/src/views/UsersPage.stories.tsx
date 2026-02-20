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
		await expect(await canvas.findByText("used/limit")).toBeInTheDocument();

		// Aggregated quota for the first user: 10 GiB + 5 GiB.
		await expect(await canvas.findByText("0/15 GiB")).toBeInTheDocument();
	},
};

export const LongValues: Story = {
	parameters: {
		mockApi: {
			data: {
				users: [
					{
						user_id: USER_ID_1,
						display_name:
							"Alice with a very long display name that should truncate nicely",
						subscription_token: `sub_${USER_ID_1}_this_is_a_very_long_token_suffix_for_ui_layout_testing`,
						priority_tier: "p3",
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: 480,
						},
					},
					{
						user_id: USER_ID_2,
						display_name: "Bob",
						subscription_token: `sub_${USER_ID_2}`,
						priority_tier: "p3",
						quota_reset: {
							policy: "monthly",
							day_of_month: 15,
							tz_offset_minutes: 480,
						},
					},
				],
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
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText(/Alice with a very long display name/),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/sub_01HF7YAT00T6RTJH6T9Z8ZPMDV/),
		).toBeInTheDocument();
	},
};

export const WithUsage: Story = {
	parameters: {
		mockApi: {
			data: {
				// Override the quota summary response so we can demo the dual-direction bar.
				quotaSummaries: {
					partial: false,
					unreachable_nodes: [],
					items: [
						{
							user_id: USER_ID_1,
							quota_limit_kind: "fixed",
							quota_limit_bytes: 15 * 2 ** 30,
							used_bytes: 4 * 2 ** 30,
							remaining_bytes: 11 * 2 ** 30,
						},
						{
							user_id: USER_ID_2,
							quota_limit_kind: "fixed",
							quota_limit_bytes: 5 * 2 ** 30,
							used_bytes: 1 * 2 ** 30,
							remaining_bytes: 4 * 2 ** 30,
						},
					],
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(await canvas.findByText("4 GiB/15 GiB")).toBeInTheDocument();
		await expect(await canvas.findByText("1 GiB/5 GiB")).toBeInTheDocument();
	},
};
