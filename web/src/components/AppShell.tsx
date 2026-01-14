import { useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";

import { fetchAdminAlerts } from "../api/adminAlerts";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { Icon } from "./Icon";
import { useUiPrefs } from "./UiPrefs";
import { clearAdminToken, readAdminToken } from "./auth";

type AppShellProps = {
	brand: { name: string; subtitle?: string };
	navItems: Array<{ label: string; to: string; icon: string }>;
	headerStatus?: ReactNode;
	children?: ReactNode;
};

type CommandPaletteState = { open: boolean };

export function AppShell({
	brand,
	navItems,
	headerStatus,
	children,
}: AppShellProps) {
	const navigate = useNavigate();
	const prefs = useUiPrefs();
	const [adminToken] = useState(() => readAdminToken());
	const [commandPalette, setCommandPalette] = useState<CommandPaletteState>({
		open: false,
	});

	const health = useQuery({
		queryKey: ["health"],
		queryFn: ({ signal }) => fetchHealth(signal),
	});

	const clusterInfo = useQuery({
		queryKey: ["clusterInfo"],
		queryFn: ({ signal }) => fetchClusterInfo(signal),
	});

	const adminAlerts = useQuery({
		queryKey: ["adminAlerts", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminAlerts(adminToken, signal),
	});

	useEffect(() => {
		const onKeyDown = (event: KeyboardEvent) => {
			if (!(event.ctrlKey || event.metaKey)) return;
			if (event.key.toLowerCase() !== "k") return;
			event.preventDefault();
			setCommandPalette({ open: true });
		};
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, []);

	const statusBadges = useMemo(() => {
		const items: ReactNode[] = [];

		const healthStatus =
			health.isSuccess && health.data?.status === "ok" ? "ok" : "error";
		items.push(
			<span
				key="health"
				className={[
					"badge badge-sm gap-1 font-mono",
					healthStatus === "ok" ? "badge-success" : "badge-error",
				].join(" ")}
				title="Backend health"
			>
				<span className="opacity-80">health</span>
				<span>{healthStatus}</span>
			</span>,
		);

		if (clusterInfo.isSuccess) {
			items.push(
				<span
					key="role"
					className="badge badge-sm gap-1 font-mono"
					title="Cluster role"
				>
					<span className="opacity-80">role</span>
					<span>{clusterInfo.data.role}</span>
				</span>,
			);
			items.push(
				<span
					key="term"
					className="badge badge-sm gap-1 font-mono"
					title="Cluster term"
				>
					<span className="opacity-80">term</span>
					<span>{clusterInfo.data.term}</span>
				</span>,
			);
		}

		if (adminAlerts.isSuccess) {
			const count = adminAlerts.data.items.length;
			const unreachable = adminAlerts.data.unreachable_nodes.length;
			const partial = adminAlerts.data.partial;
			const tone =
				count > 0 || unreachable > 0 || partial
					? "badge-warning"
					: "badge-ghost";
			items.push(
				<span
					key="alerts"
					className={["badge badge-sm gap-1 font-mono", tone].join(" ")}
					title="Admin alerts"
				>
					<span className="opacity-80">alerts</span>
					<span>{count}</span>
					{unreachable > 0 ? (
						<span className="opacity-80">+{unreachable}n</span>
					) : null}
					{partial ? <span className="opacity-80">partial</span> : null}
				</span>,
			);
		}

		return items;
	}, [
		adminAlerts.data,
		adminAlerts.isSuccess,
		clusterInfo.data,
		clusterInfo.isSuccess,
		health.data,
		health.isSuccess,
	]);

	return (
		<div className="min-h-screen bg-base-200">
			<div className="drawer lg:drawer-open">
				<input id="xp_drawer" type="checkbox" className="drawer-toggle" />
				<div className="drawer-content flex flex-col">
					<header className="navbar bg-base-100 border-b border-base-200">
						<div className="flex-none lg:hidden">
							<label
								htmlFor="xp_drawer"
								className="btn btn-ghost btn-sm"
								aria-label="Open menu"
							>
								<Icon name="tabler:menu-2" ariaLabel="Menu" />
							</label>
						</div>
						<div className="flex flex-1 items-center min-w-0 gap-3">
							<Link to="/" className="flex items-center gap-2">
								<span className="font-semibold tracking-tight">
									{brand.name}
								</span>
								{brand.subtitle ? (
									<span className="hidden sm:inline text-xs opacity-60">
										{brand.subtitle}
									</span>
								) : null}
							</Link>
							<div className="hidden md:flex flex-wrap items-center gap-2">
								{headerStatus ?? statusBadges}
							</div>
						</div>
						<div className="flex flex-none items-center gap-2">
							<button
								type="button"
								className="btn btn-ghost btn-sm"
								onClick={() => setCommandPalette({ open: true })}
							>
								<span className="hidden sm:inline">Ctrl/⌘K</span>
								<span className="sm:hidden">K</span>
							</button>

							<label className="hidden sm:flex items-center gap-2">
								<span className="text-xs opacity-60">Density</span>
								<select
									className="select select-bordered select-sm"
									value={prefs.density}
									onChange={(event) => {
										const next = event.target.value;
										prefs.setDensity(
											next === "compact" ? "compact" : "comfortable",
										);
									}}
								>
									<option value="comfortable">Comfortable</option>
									<option value="compact">Compact</option>
								</select>
							</label>

							<label className="hidden sm:flex items-center gap-2">
								<span className="text-xs opacity-60">Theme</span>
								<select
									className="select select-bordered select-sm"
									value={prefs.theme}
									onChange={(event) => {
										const next = event.target.value;
										if (
											next === "dark" ||
											next === "light" ||
											next === "system"
										) {
											prefs.setTheme(next);
										}
									}}
								>
									<option value="system">System</option>
									<option value="light">Light</option>
									<option value="dark">Dark</option>
								</select>
							</label>

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

					<main className="flex-1 p-[var(--xp-page-padding)]">
						{children ?? <Outlet />}
					</main>
				</div>
				<div className="drawer-side">
					<label
						htmlFor="xp_drawer"
						className="drawer-overlay"
						aria-label="Close menu"
					/>
					<aside className="min-h-full w-72 bg-base-100 border-r border-base-200">
						<div className="p-4">
							<div className="flex items-baseline justify-between">
								<div className="space-y-0.5">
									<p className="font-semibold tracking-tight">{brand.name}</p>
									{brand.subtitle ? (
										<p className="text-xs opacity-60">{brand.subtitle}</p>
									) : null}
								</div>
								<span className="badge badge-sm font-mono opacity-80">
									{prefs.resolvedTheme}
								</span>
							</div>
						</div>

						<nav className="px-3 pb-4">
							<ul className="menu gap-1">
								{navItems.map((item) => (
									<li key={item.to}>
										<Link
											to={item.to}
											activeProps={{ className: "active" }}
											className="rounded-lg"
										>
											<Icon name={item.icon} className="opacity-80" />
											<span>{item.label}</span>
										</Link>
									</li>
								))}
							</ul>
						</nav>

						<div className="px-4 pb-6 lg:hidden">
							<div className="space-y-3 rounded-box border border-base-200 bg-base-200 p-3">
								<div className="flex items-center justify-between gap-3">
									<span className="text-xs opacity-60">Theme</span>
									<select
										className="select select-bordered select-sm"
										value={prefs.theme}
										onChange={(event) => {
											const next = event.target.value;
											if (
												next === "dark" ||
												next === "light" ||
												next === "system"
											) {
												prefs.setTheme(next);
											}
										}}
									>
										<option value="system">System</option>
										<option value="light">Light</option>
										<option value="dark">Dark</option>
									</select>
								</div>
								<div className="flex items-center justify-between gap-3">
									<span className="text-xs opacity-60">Density</span>
									<select
										className="select select-bordered select-sm"
										value={prefs.density}
										onChange={(event) => {
											const next = event.target.value;
											prefs.setDensity(
												next === "compact" ? "compact" : "comfortable",
											);
										}}
									>
										<option value="comfortable">Comfortable</option>
										<option value="compact">Compact</option>
									</select>
								</div>
							</div>
						</div>
					</aside>
				</div>
			</div>

			<dialog className="modal" open={commandPalette.open}>
				<div className="modal-box">
					<h3 className="text-lg font-bold">Command palette</h3>
					<p className="py-4 text-sm opacity-70">
						Placeholder for Ctrl/⌘+K quick actions. (Planned in #0010.)
					</p>
					<div className="modal-action">
						<button
							type="button"
							className="btn"
							onClick={() => setCommandPalette({ open: false })}
						>
							Close
						</button>
					</div>
				</div>
				<form method="dialog" className="modal-backdrop">
					<button
						type="button"
						onClick={() => setCommandPalette({ open: false })}
					>
						close
					</button>
				</form>
			</dialog>
		</div>
	);
}
