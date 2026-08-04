import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

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

export const PrivateMirrorTargetsBlocked: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const checkbox = await canvas.findByRole("checkbox", {
			name: "Allow private Mihomo mirror targets",
		});
		await expect(checkbox).toHaveAttribute("aria-checked", "false");
		await userEvent.click(checkbox);
		await expect(await canvas.findByText("Allowed")).toBeInTheDocument();
	},
};
