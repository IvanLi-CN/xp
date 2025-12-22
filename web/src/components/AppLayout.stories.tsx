import type { Meta, StoryObj } from "@storybook/react";

import { AppLayout } from "./AppLayout";

const meta: Meta<typeof AppLayout> = {
	title: "Components/AppLayout",
	component: AppLayout,
	args: {
		children: (
			<div className="space-y-2">
				<h2 className="text-xl font-bold">Content</h2>
				<p className="text-sm opacity-70">Rendered inside AppLayout.</p>
			</div>
		),
	},
};

export default meta;

type Story = StoryObj<typeof AppLayout>;

export const Default: Story = {};
