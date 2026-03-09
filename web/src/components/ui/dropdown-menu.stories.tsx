import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuLabel,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "./dropdown-menu";

const meta = {
	title: "UI/DropdownMenu",
	component: DropdownMenu,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Menu primitive used for compact action menus. Open stories expose menu grouping, inset labels, and destructive affordances without relying on a page-level wrapper.",
			},
		},
	},
} satisfies Meta<typeof DropdownMenu>;

export default meta;

type Story = StoryObj<typeof meta>;

function MenuDemo(props: { open?: boolean }) {
	return (
		<DropdownMenu open={props.open}>
			<DropdownMenuTrigger asChild>
				<Button variant="outline">Actions</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent>
				<DropdownMenuLabel>Node actions</DropdownMenuLabel>
				<DropdownMenuItem>Edit metadata</DropdownMenuItem>
				<DropdownMenuItem>Open runtime</DropdownMenuItem>
				<DropdownMenuSeparator />
				<DropdownMenuItem className="text-destructive">
					Delete node
				</DropdownMenuItem>
			</DropdownMenuContent>
		</DropdownMenu>
	);
}

export const Closed: Story = {
	render: () => <MenuDemo />,
};

export const OpenMenu: Story = {
	render: () => <MenuDemo open />,
};
