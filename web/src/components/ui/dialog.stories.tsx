import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	DialogTrigger,
} from "./dialog";

const meta = {
	title: "UI/Dialog",
	component: Dialog,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Modal dialog primitive used by custom composite flows. Stories show a default trigger flow plus an always-open snapshot to keep docs readable and accessibility wiring visible.",
			},
		},
	},
} satisfies Meta<typeof Dialog>;

export default meta;

type Story = StoryObj<typeof meta>;

export const ClosedWithTrigger: Story = {
	render: () => (
		<Dialog>
			<DialogTrigger asChild>
				<Button>Open dialog</Button>
			</DialogTrigger>
			<DialogContent>
				<DialogHeader>
					<DialogTitle>Edit node metadata</DialogTitle>
					<DialogDescription>
						Update labels and notes before saving changes.
					</DialogDescription>
				</DialogHeader>
				<div className="rounded-xl border border-border/70 p-4 text-sm text-muted-foreground">
					Metadata form body placeholder.
				</div>
				<DialogFooter>
					<Button variant="outline">Cancel</Button>
					<Button>Save</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	),
};

export const OpenState: Story = {
	render: () => (
		<Dialog defaultOpen>
			<DialogContent>
				<DialogHeader>
					<DialogTitle>Delete endpoint?</DialogTitle>
					<DialogDescription>
						This preview captures spacing, title hierarchy, and footer
						alignment.
					</DialogDescription>
				</DialogHeader>
				<DialogFooter>
					<Button variant="outline">Cancel</Button>
					<Button variant="destructive">Delete</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	),
};
