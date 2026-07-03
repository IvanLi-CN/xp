import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const NODE_ID = "node-hinet";

const meta = {
	title: "Pages/EndpointNewPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/endpoints/new",
		},
		mockApi: {
			data: {
				nodes: [
					{
						node_id: NODE_ID,
						node_name: "hinet",
						access_host: "hinet.example.com",
						api_base_url: "https://hinet-api.example.com:62416",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
				],
				realityDomains: [],
				endpoints: [],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const ManualDestApiAddressSuggestion: Story = {};

export const ManualDestApiAddressSuggestionShowsOnEmptyFocus: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const destInput = await canvas.findByLabelText("dest");
		await expect(destInput).not.toHaveAttribute("list");
		await userEvent.click(destInput);
		await expect(
			await within(document.body).findByText("hinet-api.example.com:62416"),
		).toBeVisible();
		await userEvent.click(
			await within(document.body).findByText("hinet-api.example.com:62416"),
		);
		await expect(destInput).toHaveValue("hinet-api.example.com:62416");
	},
};
