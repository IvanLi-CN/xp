import type { Meta, StoryObj } from "@storybook/react";

import { Separator } from "./separator";

const meta = {
	title: "UI/Separator",
	component: Separator,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Visual separator primitive for dense admin surfaces. The pair of stories shows horizontal section breaks and vertical action splits used in toolbars.",
			},
		},
	},
} satisfies Meta<typeof Separator>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Horizontal: Story = {
	render: () => (
		<div className="w-[320px] space-y-3 text-sm">
			<div>Section A</div>
			<Separator />
			<div>Section B</div>
		</div>
	),
};

export const Vertical: Story = {
	render: () => (
		<div className="flex h-12 items-center gap-3 text-sm">
			<span>Filters</span>
			<Separator orientation="vertical" />
			<span>Bulk actions</span>
		</div>
	),
};
