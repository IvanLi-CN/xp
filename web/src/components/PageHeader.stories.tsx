import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./Button";
import { PageHeader } from "./PageHeader";

const meta: Meta<typeof PageHeader> = {
	title: "Components/PageHeader",
	component: PageHeader,
	args: {
		title: "Nodes",
		description: "Inspect cluster nodes and issue join tokens.",
	},
};

export default meta;

type Story = StoryObj<typeof PageHeader>;

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
				<span className="badge badge-success badge-sm font-mono">
					health ok
				</span>
				<span className="badge badge-sm font-mono">role leader</span>
			</div>
		),
	},
};
