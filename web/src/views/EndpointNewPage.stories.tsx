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

export const ManualDestApiAddressSuggestion: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const destInput = await canvas.findByLabelText("dest");
		await expect(destInput).toHaveAttribute("list", "reality-dest-suggestions");
		const suggestion = canvasElement.querySelector(
			'datalist#reality-dest-suggestions option[value="hinet-api.example.com:62416"]',
		);
		await expect(suggestion).not.toBeNull();
		await userEvent.type(destInput, "hinet-api.example.com:62416");
		await expect(destInput).toHaveValue("hinet-api.example.com:62416");
	},
};
