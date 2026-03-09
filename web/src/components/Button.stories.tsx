import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./Button";
import { Icon } from "./Icon";

const meta = {
	title: "Components/Button",
	component: Button,
	tags: ["autodocs", "coverage-ui"],
	args: {
		children: "Save changes",
	},
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Product button wrapper over shadcn/ui `Button`. `variant` maps app intent (`primary`, `secondary`, `ghost`, `danger`), `loading` swaps in the spinner, and the default size follows `UiPrefs` density unless `size` is set explicitly.",
			},
		},
	},
} satisfies Meta<typeof Button>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithIcon: Story = {
	args: {
		children: "Create endpoint",
		iconLeft: <Icon name="tabler:plus" ariaLabel="Add" />,
	},
};

export const Secondary: Story = {
	args: {
		variant: "secondary",
		children: "Refresh",
	},
};

export const Danger: Story = {
	args: {
		variant: "danger",
		children: "Delete node",
	},
};

export const Loading: Story = {
	args: {
		loading: true,
		children: "Saving",
	},
};

export const CompactExplicit: Story = {
	args: {
		size: "sm",
		variant: "ghost",
		children: "Compact action",
	},
};
