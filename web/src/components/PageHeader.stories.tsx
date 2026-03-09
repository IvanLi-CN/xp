import type { Meta, StoryObj } from "@storybook/react";

import { Badge } from "@/components/ui/badge";

import { Button } from "./Button";
import { PageHeader } from "./PageHeader";

const meta = {
	title: "Components/PageHeader",
	component: PageHeader,
	tags: ["autodocs", "coverage-ui"],
	args: {
		title: "Nodes",
		description: "Inspect cluster nodes and issue join tokens.",
	},
	parameters: {
		docs: {
			description: {
				component:
					"Section header used across app pages. Stories focus on the reusable visual contract: title, supporting description, metadata badges, and right-aligned action groups.",
			},
		},
	},
} satisfies Meta<typeof PageHeader>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithActions: Story = {
	args: {
		actions: (
			<div className="flex flex-wrap items-center gap-2">
				<Button variant="secondary">Refresh</Button>
				<Button>New</Button>
			</div>
		),
		meta: (
			<div className="flex flex-wrap items-center gap-2">
				<Badge variant="success" size="sm" className="font-mono">
					health ok
				</Badge>
				<Badge variant="outline" size="sm" className="font-mono">
					role leader
				</Badge>
			</div>
		),
	},
};
