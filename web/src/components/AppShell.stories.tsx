import type { Meta, StoryObj } from "@storybook/react";
import { useEffect } from "react";

import { AppShell } from "./AppShell";
import { useUiPrefs } from "./UiPrefs";

function SetTheme({ theme }: { theme: "light" | "dark" }) {
	const prefs = useUiPrefs();
	useEffect(() => {
		prefs.setTheme(theme);
	}, [prefs, theme]);
	return null;
}

const meta: Meta<typeof AppShell> = {
	title: "Components/AppShell",
	component: AppShell,
	args: {
		brand: { name: "xp", subtitle: "control plane" },
		navItems: [
			{ label: "Dashboard", to: "/", icon: "tabler:layout-dashboard" },
			{ label: "Nodes", to: "/nodes", icon: "tabler:server" },
			{ label: "Endpoints", to: "/endpoints", icon: "tabler:plug" },
			{ label: "Users", to: "/users", icon: "tabler:users" },
			{ label: "Grants", to: "/grants", icon: "tabler:key" },
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

export const Light: Story = {
	render: (args) => (
		<>
			<SetTheme theme="light" />
			<AppShell {...args} />
		</>
	),
};

export const Dark: Story = {
	render: (args) => (
		<>
			<SetTheme theme="dark" />
			<AppShell {...args} />
		</>
	),
};
