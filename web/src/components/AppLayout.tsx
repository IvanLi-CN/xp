import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { clearAdminToken } from "./auth";

const navItems = [
	{ label: "Dashboard", to: "/" },
	{ label: "Nodes", to: "/nodes" },
	{ label: "Endpoints", to: "/endpoints" },
	{ label: "Users", to: "/users" },
	{ label: "Grants", to: "/grants" },
];

type AppLayoutProps = {
	children?: ReactNode;
};

export function AppLayout({ children }: AppLayoutProps) {
	const navigate = useNavigate();

	return (
		<div className="min-h-screen bg-base-200">
			<header className="navbar bg-base-100 shadow">
				<div className="flex-1">
					<Link className="btn btn-ghost text-xl" to="/">
						xp
					</Link>
					<span className="text-sm opacity-70">M6 control plane</span>
				</div>
				<div className="flex-none">
					<button
						type="button"
						className="btn btn-ghost btn-sm"
						onClick={() => {
							clearAdminToken();
							navigate({ to: "/login" });
						}}
					>
						Logout
					</button>
				</div>
			</header>
			<div className="flex flex-col lg:flex-row">
				<aside className="bg-base-100 border-base-200 border-b lg:border-b-0 lg:border-r lg:min-h-[calc(100vh-4rem)] lg:w-64">
					<nav className="p-3">
						<ul className="menu menu-horizontal gap-1 lg:menu-vertical">
							{navItems.map((item) => (
								<li key={item.to}>
									<Link
										to={item.to}
										className="rounded-lg"
										activeProps={{ className: "active" }}
									>
										{item.label}
									</Link>
								</li>
							))}
						</ul>
					</nav>
				</aside>
				<main className="flex-1 p-6">{children ?? <Outlet />}</main>
			</div>
		</div>
	);
}
