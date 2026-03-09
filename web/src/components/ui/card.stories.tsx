import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./button";
import {
	Card,
	CardContent,
	CardDescription,
	CardFooter,
	CardHeader,
	CardTitle,
} from "./card";

const meta = {
	title: "UI/Card",
	component: Card,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Card primitive for grouping dense admin information. The stories show a compact summary shell and a more content-heavy layout so theme and density changes stay visible in docs.",
			},
		},
	},
} satisfies Meta<typeof Card>;

export default meta;

type Story = StoryObj<typeof meta>;

export const SummaryCard: Story = {
	render: () => (
		<Card className="w-[360px]">
			<CardHeader>
				<CardTitle>Tokyo node</CardTitle>
				<CardDescription>
					Healthy runtime with zero quota drift.
				</CardDescription>
			</CardHeader>
			<CardContent className="space-y-2 text-sm text-muted-foreground">
				<p>API base: https://tokyo.example.com</p>
				<p>Leader term: 12</p>
			</CardContent>
			<CardFooter className="justify-end gap-2">
				<Button variant="outline" size="sm">
					Details
				</Button>
				<Button size="sm">Open</Button>
			</CardFooter>
		</Card>
	),
};

export const DenseContent: Story = {
	render: () => (
		<Card className="w-[420px]">
			<CardHeader>
				<CardTitle>Reality domains</CardTitle>
				<CardDescription>
					Edge state with multi-line metadata and action footer.
				</CardDescription>
			</CardHeader>
			<CardContent className="space-y-3 text-sm">
				<div className="rounded-xl border border-border/70 bg-muted/40 p-3">
					api.example.com {"->"} node-tokyo
				</div>
				<div className="rounded-xl border border-border/70 bg-muted/40 p-3">
					cdn.example.com {"->"} node-osaka
				</div>
			</CardContent>
		</Card>
	),
};
