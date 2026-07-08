import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

import { ReadStateBanner } from "./ReadStateBanner";

const meta: Meta<typeof ReadStateBanner> = {
	title: "Components/ReadStateBanner",
	component: ReadStateBanner,
	tags: ["autodocs", "coverage-ui"],
	args: {
		title: "Offline read-only mode is active",
		description:
			"Showing the most recently synced cluster snapshot. Writes stay " +
			"disabled until the connection returns.",
	},
};

export default meta;

type Story = StoryObj<typeof ReadStateBanner>;

export const OfflineSnapshot: Story = {};

export const Reconnecting: Story = {
	args: {
		title: "Live status stream is reconnecting",
		description:
			"The dashboard stays on the latest cached status until the next snapshot arrives.",
	},
};

export const InlineDismissible: Story = {
	args: {
		variant: "inline",
		dismissible: true,
		title: "Offline node snapshot",
		description: "Last successful sync: 2026/07/08 16:34:04.",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", {
				name: /dismiss offline node snapshot/i,
			}),
		);
		await expect(
			canvas.queryByText(/offline node snapshot/i),
		).not.toBeInTheDocument();
	},
};
