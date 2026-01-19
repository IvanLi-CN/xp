import type { ReactNode } from "react";

import { AppShell } from "./AppShell";

type AppLayoutProps = {
	children?: ReactNode;
};

export function AppLayout({ children }: AppLayoutProps) {
	return (
		<AppShell
			brand={{ name: "xp", subtitle: "control plane" }}
			navGroups={[
				{
					title: "Nav",
					items: [
						{ label: "Dashboard", to: "/", icon: "tabler:layout-dashboard" },
						{ label: "Nodes", to: "/nodes", icon: "tabler:server" },
						{ label: "Endpoints", to: "/endpoints", icon: "tabler:plug" },
						{ label: "Users", to: "/users", icon: "tabler:users" },
						{ label: "Grants", to: "/grants", icon: "tabler:key" },
					],
				},
				{
					title: "Settings",
					items: [
						{
							label: "Service config",
							to: "/service-config",
							icon: "tabler:settings",
						},
					],
				},
			]}
		>
			{children}
		</AppShell>
	);
}
