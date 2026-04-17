import type { Meta, StoryObj } from "@storybook/react";
import { expect, screen, userEvent, within } from "@storybook/test";

const meta = {
	title: "Pages/ServiceConfigPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/service-config",
		},
		mockApi: {
			data: {
				mihomoDeliveryMode: "legacy",
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const ProviderDefault: Story = {
	parameters: {
		mockApi: {
			data: {
				mihomoDeliveryMode: "provider",
			},
		},
	},
};

export const SaveProviderMode: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText("Current default: legacy"),
		).toBeInTheDocument();
		await userEvent.click(
			await canvas.findByLabelText("Mihomo default delivery"),
		);
		await userEvent.click(
			await screen.findByRole("option", { name: "provider" }),
		);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Save default route" }),
		);
		await expect(
			await canvas.findByText("Current default: provider"),
		).toBeInTheDocument();
	},
};
