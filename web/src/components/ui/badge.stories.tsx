import type { Meta, StoryObj } from "@storybook/react";

import { Badge } from "./badge";

const meta = {
	title: "UI/Badge",
	component: Badge,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Status and metadata badge primitive used across cards, tables, and runtime surfaces. Check semantic variants in both light/dark themes to confirm contrast stays readable.",
			},
		},
	},
} satisfies Meta<typeof Badge>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Variants: Story = {
	render: () => (
		<div className="flex flex-wrap gap-2">
			<Badge>default</Badge>
			<Badge variant="secondary">secondary</Badge>
			<Badge variant="success">success</Badge>
			<Badge variant="warning">warning</Badge>
			<Badge variant="destructive">destructive</Badge>
			<Badge variant="info">info</Badge>
			<Badge variant="ghost">ghost</Badge>
			<Badge variant="outline">outline</Badge>
		</div>
	),
};

export const Compact: Story = {
	render: () => (
		<div className="flex flex-wrap gap-2">
			<Badge size="sm">compact</Badge>
			<Badge size="sm" variant="success">
				healthy
			</Badge>
		</div>
	),
};
