import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

const USER_ID_1 = fixtureCatalog.identifier.userPrimary();
const USER_ID_2 = fixtureCatalog.identifier.userSecondary();

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
						node_id: fixtureCatalog.nodeId.fixture32(),
						quota_limit_bytes: fixtureCatalog.quota.tenGiB(),
						quota_reset_source: "user",
					},
					{
						user_id: USER_ID_1,
						node_id: fixtureCatalog.nodeId.fixture36(),
						quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
						quota_reset_source: "user",
					},
					{
						user_id: USER_ID_2,
						node_id: fixtureCatalog.nodeId.fixture36(),
						quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
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
						subscription_token: fixtureCatalog.token.fixture170(),
						credential_epoch: 0,
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
						subscription_token: fixtureCatalog.token.fixture171(),
						credential_epoch: 0,
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
						node_id: fixtureCatalog.nodeId.fixture32(),
						quota_limit_bytes: fixtureCatalog.quota.tenGiB(),
						quota_reset_source: "user",
					},
					{
						user_id: USER_ID_1,
						node_id: fixtureCatalog.nodeId.fixture36(),
						quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
						quota_reset_source: "user",
					},
					{
						user_id: USER_ID_2,
						node_id: fixtureCatalog.nodeId.fixture36(),
						quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
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
			await canvas.findByText(fixtureCatalog.token.fixture170()),
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
							quota_limit_bytes: fixtureCatalog.quota.fifteenGiB(),
							used_bytes: fixtureCatalog.quota.fourGiB(),
							remaining_bytes: fixtureCatalog.quota.elevenGiB(),
						},
						{
							user_id: USER_ID_2,
							quota_limit_kind: "fixed",
							quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
							used_bytes: fixtureCatalog.quota.oneGiB(),
							remaining_bytes: fixtureCatalog.quota.fourGiB(),
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
