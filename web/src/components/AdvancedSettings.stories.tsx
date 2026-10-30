import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { AdvancedSettings } from "./AdvancedSettings";

const meta = {
	title: "Components/AdvancedSettings",
	component: AdvancedSettings,
	tags: ["autodocs"],
	args: {
		title: "Advanced settings",
		children: (
			<p className="py-3 text-sm">
				Additional settings belong inside this panel.
			</p>
		),
	},
} satisfies Meta<typeof AdvancedSettings>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Collapsed: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(canvas.getByText("Advanced settings"));
		await expect(
			canvas.getByText("Additional settings belong inside this panel."),
		).toBeVisible();
	},
};

export const Expanded: Story = { args: { defaultOpen: true } };
