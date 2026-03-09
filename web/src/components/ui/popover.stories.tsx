import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./button";
import { Popover, PopoverContent, PopoverTrigger } from "./popover";

const meta = {
	title: "UI/Popover",
	component: Popover,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Popover primitive for compact contextual help and inline editors. The open snapshot makes spacing and border treatments visible in docs without extra interaction.",
			},
		},
	},
} satisfies Meta<typeof Popover>;

export default meta;

type Story = StoryObj<typeof meta>;

function PopoverDemo(props: { open?: boolean }) {
	return (
		<Popover open={props.open}>
			<PopoverTrigger asChild>
				<Button variant="outline">Open popover</Button>
			</PopoverTrigger>
			<PopoverContent>
				<div className="space-y-1">
					<p className="font-medium">Quota note</p>
					<p className="text-sm text-muted-foreground">
						Reset values follow node-local timezone defaults unless explicitly
						overridden.
					</p>
				</div>
			</PopoverContent>
		</Popover>
	);
}

export const Closed: Story = {
	render: () => <PopoverDemo />,
};

export const OpenPopover: Story = {
	render: () => <PopoverDemo open />,
};
