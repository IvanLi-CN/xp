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

function safeHostFromUrl(value: string): string | null {
	try {
		const url = new URL(value);
		return url.host || null;
	} catch {
		return null;
	}
}

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
	const [mobileNavOpen, setMobileNavOpen] = useState(false);

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

		const healthOk = health.isSuccess && health.data?.status === "ok";
		items.push(
			<span
				key="health"
				className={[
					"badge badge-sm gap-2 font-mono",
					healthOk ? "badge-info" : "badge-error",
				].join(" ")}
				title="Backend health"
			>
				<span>Health</span>
				<span className="opacity-80">{healthOk ? "OK" : "error"}</span>
			</span>,
		);

		if (clusterInfo.isSuccess) {
			const leaderHost = safeHostFromUrl(clusterInfo.data.leader_api_base_url);
			const leaderValue =
				clusterInfo.data.role === "leader"
					? clusterInfo.data.node_id
					: (leaderHost ?? clusterInfo.data.leader_api_base_url);

			items.push(
				<span
					key="leader"
					className="badge badge-sm gap-2 font-mono badge-ghost"
					title="Cluster leader"
				>
					<span>leader:</span>
					<span className="opacity-80">{leaderValue}</span>
				</span>,
			);
			items.push(
				<span
					key="term"
					className="badge badge-sm gap-2 font-mono badge-ghost"
					title="Cluster term"
				>
					<span>term:</span>
					<span className="opacity-80">{clusterInfo.data.term}</span>
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
					className={["badge badge-sm gap-2 font-mono", tone].join(" ")}
					title="Admin alerts"
				>
					<span>alerts:</span>
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

	const navContent = (
		<nav
			aria-label="Primary navigation"
			className="rounded-box border border-base-200 bg-base-100 p-4"
		>
			<p className="px-2 pb-3 text-xs uppercase tracking-wide opacity-50">
				NAV
			</p>
			<ul className="space-y-2">
				{navItems.map((item) => (
					<li key={item.to}>
						<Link
							to={item.to}
							className="flex items-center gap-3 rounded-box px-3 py-2 border border-base-200 bg-base-100 hover:bg-base-200 transition-colors"
							activeProps={{
								className:
									"bg-info/10 border-info/30 text-base-content hover:bg-info/10",
							}}
						>
							<Icon name={item.icon} className="size-5 opacity-80" />
							<span className="font-medium">{item.label}</span>
						</Link>
					</li>
				))}
			</ul>
		</nav>
	);

	return (
		<div className="min-h-screen bg-base-200">
			<div className="p-4">
				<div className="mx-auto max-w-7xl space-y-4">
					<header className="rounded-box border border-base-200 bg-base-100 px-4 py-3">
						<div className="grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-4">
							<div className="flex items-center gap-3 min-w-0">
								<button
									type="button"
									className="btn btn-ghost btn-sm lg:hidden"
									aria-label="Open menu"
									onClick={() => setMobileNavOpen(true)}
								>
									<Icon name="tabler:menu-2" ariaLabel="Menu" />
								</button>

								<Link to="/" className="flex items-baseline gap-2 min-w-0">
									<span className="font-semibold tracking-tight text-lg">
										{brand.name}
									</span>
									{brand.subtitle ? (
										<span className="hidden sm:inline text-sm opacity-60 truncate">
											{brand.subtitle}
										</span>
									) : null}
								</Link>
							</div>

							<div className="hidden lg:flex items-center justify-center gap-4">
								<div className="w-full max-w-md relative">
									<input
										type="text"
										readOnly
										placeholder="Search / command palette"
										className="input input-bordered w-full pr-20 bg-base-200/40 cursor-pointer"
										onClick={() => setCommandPalette({ open: true })}
										onFocus={() => setCommandPalette({ open: true })}
									/>
									<div className="absolute right-2 top-1/2 -translate-y-1/2 flex items-center gap-1 pointer-events-none">
										<kbd className="kbd kbd-sm">Ctrl</kbd>
										<kbd className="kbd kbd-sm">K</kbd>
									</div>
								</div>

								<div className="flex items-center gap-2">
									{headerStatus ?? statusBadges}
								</div>
							</div>

							<div className="flex items-center justify-end gap-2">
								<div className="dropdown dropdown-end">
									<button
										type="button"
										className="btn btn-outline btn-sm xp-btn-outline"
									>
										Theme
									</button>
									<div className="dropdown-content z-[1] w-72 rounded-box border border-base-200 bg-base-100 shadow">
										<div className="p-3 space-y-3">
											<div className="space-y-1">
												<p className="text-xs uppercase tracking-wide opacity-60">
													Theme
												</p>
												<select
													className="select select-bordered select-sm w-full"
													aria-label="Theme"
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
											<div className="space-y-1">
												<p className="text-xs uppercase tracking-wide opacity-60">
													Density
												</p>
												<select
													className="select select-bordered select-sm w-full"
													aria-label="Density"
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
											<div className="border-t border-base-200 pt-3">
												<button
													type="button"
													className="btn btn-ghost btn-sm w-full justify-start"
													onClick={() => setCommandPalette({ open: true })}
												>
													Command palette
													<span className="ml-auto font-mono text-xs opacity-60">
														Ctrl/⌘K
													</span>
												</button>
											</div>
										</div>
									</div>
								</div>

								<button
									type="button"
									className="btn btn-outline btn-sm xp-btn-outline"
									onClick={() => {
										clearAdminToken();
										navigate({ to: "/login" });
									}}
								>
									Logout
								</button>
							</div>
						</div>
					</header>

					<div className="grid gap-6 lg:grid-cols-[17rem_minmax(0,1fr)]">
						<aside>{navContent}</aside>
						<main className="rounded-box border border-base-200 bg-base-100 p-[var(--xp-page-padding)]">
							{children ?? <Outlet />}
						</main>
					</div>
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

			<dialog className="modal" open={mobileNavOpen}>
				<div className="modal-box p-0">
					<div className="p-4 flex items-center justify-between">
						<p className="font-semibold tracking-tight">{brand.name}</p>
						<button
							type="button"
							className="btn btn-ghost btn-sm"
							onClick={() => setMobileNavOpen(false)}
						>
							Close
						</button>
					</div>
					<div className="px-4 pb-4">{navContent}</div>
				</div>
				<form method="dialog" className="modal-backdrop">
					<button type="button" onClick={() => setMobileNavOpen(false)}>
						close
					</button>
				</form>
			</dialog>
		</div>
	);
}
