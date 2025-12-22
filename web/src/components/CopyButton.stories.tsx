import type { Meta, StoryObj } from "@storybook/react";

import { CopyButton } from "./CopyButton";

const meta: Meta<typeof CopyButton> = {
	title: "Components/CopyButton",
	component: CopyButton,
	args: {
		text: "https://example.com",
	},
};

export default meta;

type Story = StoryObj<typeof CopyButton>;

export const Default: Story = {};
