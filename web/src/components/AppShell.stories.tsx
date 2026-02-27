import type { Meta, StoryObj } from "@storybook/react";

import { AppShell } from "./AppShell";

const meta: Meta<typeof AppShell> = {
	title: "Components/AppShell",
	component: AppShell,
	args: {
		brand: { name: "xp", subtitle: "cluster manager" },
		navItems: [
			{ label: "Dashboard", to: "/", icon: "tabler:layout-dashboard" },
			{ label: "Nodes", to: "/nodes", icon: "tabler:server" },
			{ label: "Endpoints", to: "/endpoints", icon: "tabler:plug" },
			{ label: "Users", to: "/users", icon: "tabler:users" },
		],
		children: (
			<div className="space-y-2">
				<h2 className="text-xl font-semibold">Content</h2>
				<p className="text-sm opacity-70">Rendered inside AppShell.</p>
			</div>
		),
	},
};

export default meta;

type Story = StoryObj<typeof AppShell>;

export const Default: Story = {};
