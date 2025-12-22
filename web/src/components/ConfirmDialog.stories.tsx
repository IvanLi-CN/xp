import type { Meta, StoryObj } from "@storybook/react";

import { ConfirmDialog } from "./ConfirmDialog";

const meta: Meta<typeof ConfirmDialog> = {
	title: "Components/ConfirmDialog",
	component: ConfirmDialog,
	args: {
		open: true,
		title: "Are you sure?",
		description: "This action cannot be undone.",
		confirmLabel: "Delete",
		cancelLabel: "Cancel",
	},
};

export default meta;

type Story = StoryObj<typeof ConfirmDialog>;

export const Default: Story = {};
