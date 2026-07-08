import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, userEvent, within } from "@storybook/test";

import { PwaUpdateNotice } from "./PwaUpdateNotice";

const meta: Meta<typeof PwaUpdateNotice> = {
	title: "Components/PwaUpdateNotice",
	component: PwaUpdateNotice,
	tags: ["autodocs", "coverage-ui"],
	args: {
		onClose: fn(),
		onReload: fn(),
	},
	parameters: {
		layout: "fullscreen",
	},
};

export default meta;

type Story = StoryObj<typeof PwaUpdateNotice>;

export const Default: Story = {};

export const Actions: Story = {
	play: async ({ args, canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByRole("button", { name: "Close" }));
		await expect(args.onClose).toHaveBeenCalledTimes(1);

		await userEvent.click(
			await canvas.findByRole("button", { name: "Reload" }),
		);
		await expect(args.onReload).toHaveBeenCalledTimes(1);
	},
};
