import type { Meta, StoryObj } from "@storybook/react";
import { expect, screen, userEvent, within } from "@storybook/test";

import type { AdminNode } from "../api/adminNodes";
import { buildDenseUserIpUsageStories } from "../storybook/ipUsageStoryData";

const USER_ID_1 = "01HF7YAT00T6RTJH6T9Z8ZPMDV";
const USER_ID_2 = "01HF7YAT01YVKWQ847J5T9EY84";
const TOKYO_NODE_ID = "node-1";
const OSAKA_NODE_ID = "node-2";

const nodes: AdminNode[] = [
	{
		node_id: TOKYO_NODE_ID,
		node_name: "tokyo-1",
		access_host: "tokyo-1.example.com",
		api_base_url: "https://tokyo-1.example.com",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		},
	},
	{
		node_id: OSAKA_NODE_ID,
		node_name: "osaka-1",
		access_host: "osaka-1.example.com",
		api_base_url: "https://osaka-1.example.com",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		},
	},
];

const userUsageReports = buildDenseUserIpUsageStories(
	{
		user_id: USER_ID_1,
		display_name: "Alice",
	},
	nodes,
);

const meta = {
	title: "Pages/UserDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: `/users/${USER_ID_1}`,
		},
		mockApi: {
			data: {
				nodes,
				userAccessByUserId: {
					[USER_ID_1]: [
						{
							user_id: USER_ID_1,
							endpoint_id: "endpoint-1",
							node_id: TOKYO_NODE_ID,
						},
						{
							user_id: USER_ID_1,
							endpoint_id: "endpoint-2",
							node_id: OSAKA_NODE_ID,
						},
					],
				},
				nodeQuotas: [
					{
						user_id: USER_ID_1,
						node_id: TOKYO_NODE_ID,
						quota_limit_bytes: 10 * 2 ** 30,
						quota_reset_source: "user",
					},
					{
						user_id: USER_ID_1,
						node_id: OSAKA_NODE_ID,
						quota_limit_bytes: 5 * 2 ** 30,
						quota_reset_source: "node",
					},
				],
				userIpUsageByUserId: {
					[USER_ID_1]: userUsageReports,
				},
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
		await expect(
			await canvas.findByText(
				"After Apply access, new VLESS and SS2022 endpoints will be assigned to this user automatically.",
			),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(
				"Node all-select covers current endpoints on tokyo-1 and osaka-1 only. Future endpoints still follow protocol all-select defaults.",
			),
		).toBeInTheDocument();
	},
};

export const QuotaStatusTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Quota status" }),
		);
		await expect(
			(await canvas.findAllByText(/Remaining:/)).length,
		).toBeGreaterThan(0);
	},
};

export const UsageDetailsTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Usage details" }),
		);
		await expect(
			await canvas.findByRole("tab", { name: "tokyo-1" }),
		).toHaveAttribute("aria-selected", "true");
		await expect(
			await canvas.findByText("Usage details · tokyo-1"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", { name: "198.51.100.57" }),
		).toBeInTheDocument();
	},
};

export const UsageDetailsDuplicateNames: Story = {
	parameters: {
		mockApi: {
			data: {
				nodes: [
					{
						node_id: "dup-node-a",
						node_name: "tokyo",
						access_host: "tokyo-a.example.com",
						api_base_url: "https://tokyo-a.example.com",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
					{
						node_id: "dup-node-b",
						node_name: "tokyo",
						access_host: "tokyo-b.example.com",
						api_base_url: "https://tokyo-b.example.com",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
				],
				userIpUsageByUserId: {
					[USER_ID_1]: {
						...buildDenseUserIpUsageStories(
							{
								user_id: USER_ID_1,
								display_name: "Alice",
							},
							[
								{
									node_id: "dup-node-a",
									node_name: "tokyo",
									access_host: "tokyo-a.example.com",
									api_base_url: "https://tokyo-a.example.com",
									quota_limit_bytes: 0,
									quota_reset: {
										policy: "monthly",
										day_of_month: 1,
										tz_offset_minutes: null,
									},
								},
								{
									node_id: "dup-node-b",
									node_name: "tokyo",
									access_host: "tokyo-b.example.com",
									api_base_url: "https://tokyo-b.example.com",
									quota_limit_bytes: 0,
									quota_reset: {
										policy: "monthly",
										day_of_month: 1,
										tz_offset_minutes: null,
									},
								},
							],
						),
					},
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Usage details" }),
		);
		await expect(
			await canvas.findByRole("tab", { name: "tokyo · tokyo-a.example.com" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("tab", { name: "tokyo · tokyo-b.example.com" }),
		).toBeInTheDocument();
	},
};

export const UsageDetailsTab7d: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Usage details" }),
		);
		await userEvent.click(await canvas.findByRole("tab", { name: "osaka-1" }));
		await expect(
			await canvas.findByRole("heading", { name: "Usage details · osaka-1" }),
		).toBeInTheDocument();
		await userEvent.click(await canvas.findByRole("button", { name: "7d" }));
		await expect(
			await canvas.findByRole("button", { name: "7d" }),
		).toHaveAttribute("aria-pressed", "true");
		await expect(
			await canvas.findByRole("button", { name: "198.51.100.99" }),
		).toBeInTheDocument();
	},
};

export const MihomoProviderPreview: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByLabelText("Subscription format"));
		await userEvent.click(
			await screen.findByRole("option", { name: "mihomo(provider)" }),
		);
		await userEvent.click(await canvas.findByRole("button", { name: "Fetch" }));
		await expect(
			await screen.findByText("Subscription preview"),
		).toBeInTheDocument();
		await expect(
			await screen.findByText(/xp-system-generated/i),
		).toBeInTheDocument();
	},
};
