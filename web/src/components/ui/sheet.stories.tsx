import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./button";
import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetFooter,
	SheetHeader,
	SheetTitle,
	SheetTrigger,
} from "./sheet";

const meta = {
	title: "UI/Sheet",
	component: Sheet,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Sliding sheet primitive for mobile navigation and side editors. Stories document the default trigger and a left-side variant to catch layout drift in compact density.",
			},
		},
	},
} satisfies Meta<typeof Sheet>;

export default meta;

type Story = StoryObj<typeof meta>;

export const WithTrigger: Story = {
	render: () => (
		<Sheet>
			<SheetTrigger asChild>
				<Button>Open mobile nav</Button>
			</SheetTrigger>
			<SheetContent>
				<SheetHeader>
					<SheetTitle>Navigation</SheetTitle>
					<SheetDescription>
						Use this surface for mobile routes and quick actions.
					</SheetDescription>
				</SheetHeader>
				<div className="space-y-2 text-sm">
					<div className="rounded-xl border border-border/70 p-3">
						Dashboard
					</div>
					<div className="rounded-xl border border-border/70 p-3">Nodes</div>
					<div className="rounded-xl border border-border/70 p-3">Users</div>
				</div>
			</SheetContent>
		</Sheet>
	),
};

export const LeftSide: Story = {
	render: () => (
		<Sheet defaultOpen>
			<SheetContent side="left">
				<SheetHeader>
					<SheetTitle>Filters</SheetTitle>
					<SheetDescription>
						Edge case for wide content pinned to the left edge.
					</SheetDescription>
				</SheetHeader>
				<div className="space-y-3 text-sm">
					<p>Environment: production</p>
					<p>Status: partial</p>
				</div>
				<SheetFooter>
					<Button variant="outline">Reset</Button>
					<Button>Apply</Button>
				</SheetFooter>
			</SheetContent>
		</Sheet>
	),
};
