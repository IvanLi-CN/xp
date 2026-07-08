import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const NODE_ID = "node-alpha";

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
						node_name: "alpha",
						access_host: "node-xp.example.test",
						api_base_url: "https://node-xp.example.test:443",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
				],
				endpoints: [],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const ManagedDefaultFieldsVisible: Story = {
	tags: ["coverage-ui"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("heading", { name: "New endpoint" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByLabelText("canaryUpstreamUrl"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByLabelText("canary upstream mode"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByLabelText("accepted host[:port]"),
		).toBeInTheDocument();
		await expect(canvas.queryByLabelText("dest")).toBeNull();
		await expect(canvas.queryByLabelText("serverNames")).toBeNull();
	},
};

export const ManagedDefaultAcceptedHostDefaultsTo443: Story = {
	tags: ["coverage-ui"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const input = await canvas.findByLabelText("accepted host[:port]");
		await userEvent.type(input, "edge.example.com{enter}");
		await expect(
			await canvas.findByText("edge.example.com:443"),
		).toBeInTheDocument();
	},
};
