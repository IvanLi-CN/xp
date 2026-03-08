import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const meta = {
	title: "Pages/NodeDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/nodes/01J000000000000000000000001",
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
			await canvas.findByText("203.0.113.7", { selector: "td" }),
		).toBeInTheDocument();
		await userEvent.click(await canvas.findByRole("button", { name: "7d" }));
		await expect(
			await canvas.findByText("203.0.113.7", { selector: "td" }),
		).toBeInTheDocument();
	},
};
