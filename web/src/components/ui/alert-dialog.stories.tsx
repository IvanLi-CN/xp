import type { Meta, StoryObj } from "@storybook/react";

import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
	AlertDialogTrigger,
} from "./alert-dialog";
import { Button } from "./button";

const meta = {
	title: "UI/AlertDialog",
	component: AlertDialog,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Destructive confirmation primitive for dangerous actions. Stories document the trigger flow and the fully open confirmation state used by delete and reset actions in the app.",
			},
		},
	},
} satisfies Meta<typeof AlertDialog>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Triggered: Story = {
	render: () => (
		<AlertDialog>
			<AlertDialogTrigger asChild>
				<Button variant="destructive">Delete node</Button>
			</AlertDialogTrigger>
			<AlertDialogContent>
				<AlertDialogHeader>
					<AlertDialogTitle>Delete node?</AlertDialogTitle>
					<AlertDialogDescription>
						This action removes the node from the cluster inventory and cannot
						be undone.
					</AlertDialogDescription>
				</AlertDialogHeader>
				<AlertDialogFooter>
					<AlertDialogCancel>Cancel</AlertDialogCancel>
					<AlertDialogAction>Delete</AlertDialogAction>
				</AlertDialogFooter>
			</AlertDialogContent>
		</AlertDialog>
	),
};

export const OpenState: Story = {
	render: () => (
		<AlertDialog defaultOpen>
			<AlertDialogContent>
				<AlertDialogHeader>
					<AlertDialogTitle>Reset quotas for this node?</AlertDialogTitle>
					<AlertDialogDescription>
						Use this state to verify spacing, overlay contrast, and action
						priority.
					</AlertDialogDescription>
				</AlertDialogHeader>
				<AlertDialogFooter>
					<AlertDialogCancel>Keep current value</AlertDialogCancel>
					<AlertDialogAction>Reset now</AlertDialogAction>
				</AlertDialogFooter>
			</AlertDialogContent>
		</AlertDialog>
	),
};
