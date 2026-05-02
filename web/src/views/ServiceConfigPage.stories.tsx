import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

const meta = {
	title: "Pages/ServiceConfigPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/service-config",
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const ProviderOnly: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText(/Mihomo uses provider-only delivery/),
		).toBeInTheDocument();
		await expect(canvas.queryByText("Mihomo delivery")).not.toBeInTheDocument();
	},
};
