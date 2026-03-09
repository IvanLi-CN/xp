import type { Meta, StoryObj } from "@storybook/react";

import { ScrollArea } from "./scroll-area";

const meta = {
	title: "UI/ScrollArea",
	component: ScrollArea,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Scroll container primitive used where bounded panes need consistent scroll chrome. The story intentionally overflows so reviewers can inspect scrollbar treatment in both themes.",
			},
		},
	},
} satisfies Meta<typeof ScrollArea>;

export default meta;

type Story = StoryObj<typeof meta>;

export const LongList: Story = {
	render: () => (
		<ScrollArea className="h-56 w-[320px] rounded-2xl border border-border/70 p-4">
			<div className="space-y-2">
				{Array.from({ length: 20 }, (_, index) => {
					const itemId = `node-event-${index + 1}`;
					return (
						<div
							key={itemId}
							className="rounded-xl border border-border/60 px-3 py-2 text-sm"
						>
							Node event #{index + 1}
						</div>
					);
				})}
			</div>
		</ScrollArea>
	),
};
