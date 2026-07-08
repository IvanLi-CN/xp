import type { Meta, StoryObj } from "@storybook/react";

import { ReadStateIndicator } from "./ReadStateIndicator";

const meta: Meta<typeof ReadStateIndicator> = {
	title: "Components/ReadStateIndicator",
	component: ReadStateIndicator,
	tags: ["autodocs", "coverage-ui"],
	args: {
		tone: "warning",
		label: "Offline cached",
		title:
			"Showing cached admin data. Last successful sync: 2026/07/08 17:29:26.",
	},
};

export default meta;

type Story = StoryObj<typeof ReadStateIndicator>;

export const OfflineCached: Story = {};

export const CachedView: Story = {
	args: {
		tone: "info",
		label: "Cached view",
		title:
			"Live status is reconnecting. Last successful sync: 2026/07/08 17:29:26.",
	},
};
