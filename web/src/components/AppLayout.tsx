import type { ReactNode } from "react";

import { AppShell } from "./AppShell";

type AppLayoutProps = {
	children?: ReactNode;
};

export function AppLayout({ children }: AppLayoutProps) {
	return (
		<AppShell
			brand={{
				name: "xp",
				subtitle: "cluster manager",
				markSrc: "/xp-mark.png",
			}}
			navGroups={[
				{
					title: "Nav",
					items: [
						{ label: "Dashboard", to: "/", icon: "tabler:layout-dashboard" },
						{ label: "Nodes", to: "/nodes", icon: "tabler:server" },
						{ label: "Endpoints", to: "/endpoints", icon: "tabler:plug" },
						{ label: "Users", to: "/users", icon: "tabler:users" },
						{ label: "Grant groups", to: "/grant-groups", icon: "tabler:key" },
					],
				},
				{
					title: "Settings",
					items: [
						{
							label: "Quota policy",
							to: "/quota-policy",
							icon: "tabler:gauge",
						},
						{
							label: "Reality domains",
							to: "/reality-domains",
							icon: "tabler:world",
						},
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
