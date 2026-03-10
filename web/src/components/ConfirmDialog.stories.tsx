import type { Meta, StoryObj } from "@storybook/react";

import { ConfirmDialog } from "./ConfirmDialog";

const meta: Meta<typeof ConfirmDialog> = {
	title: "Components/ConfirmDialog",
	component: ConfirmDialog,
	tags: ["autodocs", "coverage-ui"],
	args: {
		open: true,
		title: "Are you sure?",
		description: "This action cannot be undone.",
		confirmLabel: "Delete",
		cancelLabel: "Cancel",
	},
	parameters: {
		docs: {
			description: {
				component:
					"Confirmation dialog for destructive or irreversible actions. Stories cover the default destructive copy, a long-running confirm label, and the no-description edge state used when the title alone is enough context.",
			},
		},
	},
};

export default meta;

type Story = StoryObj<typeof ConfirmDialog>;

export const Default: Story = {};

export const BusyConfirm: Story = {
	args: {
		confirmLabel: "Deleting...",
		cancelLabel: "Back",
	},
};

export const NoDescription: Story = {
	args: {
		title: "Reset subscription token",
		description: undefined,
		confirmLabel: "Reset",
	},
};
