import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./button";

const meta = {
	title: "UI/Button",
	component: Button,
	tags: ["autodocs", "coverage-ui"],
	args: {
		children: "Save changes",
		variant: "default",
		size: "default",
	},
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Core shadcn/ui button primitive. Use the Storybook theme and density toolbar to verify how button sizing, emphasis, and icon-only affordances behave across the app shell.",
			},
		},
	},
} satisfies Meta<typeof Button>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Outline: Story = {
	args: {
		variant: "outline",
		children: "Secondary action",
	},
};

export const Destructive: Story = {
	args: {
		variant: "destructive",
		children: "Delete node",
	},
};

export const IconOnly: Story = {
	args: {
		size: "icon",
		"aria-label": "Add endpoint",
		children: "+",
	},
};
