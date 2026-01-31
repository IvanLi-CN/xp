import { useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useReducer, useState } from "react";

import { fetchAdminAlerts } from "../api/adminAlerts";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { fetchVersionCheck } from "../api/versionCheck";
import { Icon } from "./Icon";
import { useUiPrefs } from "./UiPrefs";
import { clearAdminToken, readAdminToken } from "./auth";
import {
	githubReleaseTagUrl,
	readVersionCheckLastAtMs,
	reduceVersionCheckUiState,
	shouldAutoCheckVersion,
	writeVersionCheckLastAtMs,
	xpVersionLinkHref,
} from "./versionCheckUi";

type AppShellProps = {
	brand: { name: string; subtitle?: string };
	navItems?: Array<{ label: string; to: string; icon: string }>;
	navGroups?: Array<{
		title: string;
		items: Array<{ label: string; to: string; icon: string }>;
	}>;
	headerStatus?: ReactNode;
	children?: ReactNode;
};

type CommandPaletteState = { open: boolean };
type VersionCheckUiState = Parameters<typeof reduceVersionCheckUiState>[0];

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
	navGroups,
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
	const [versionCheck, dispatchVersionCheck] = useReducer(
		reduceVersionCheckUiState,
		{ kind: "idle" } satisfies VersionCheckUiState,
	);
	const [versionCheckLastAtMs, setVersionCheckLastAtMs] = useState<
		number | null
	>(() => readVersionCheckLastAtMs());

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

	const runVersionCheck = useCallback(
		async (options?: { force?: boolean }) => {
			const force = options?.force ?? false;
			const nowMs = Date.now();
			const canRun =
				force ||
				shouldAutoCheckVersion({ nowMs, lastAtMs: versionCheckLastAtMs });
			if (!canRun) return;

			writeVersionCheckLastAtMs(nowMs);
			setVersionCheckLastAtMs(nowMs);
			dispatchVersionCheck({ type: "start" });

			try {
				const data = await fetchVersionCheck();
				dispatchVersionCheck({ type: "success", data });
			} catch (err) {
				const message = isBackendApiError(err)
					? `request failed: ${err.status}`
					: err instanceof Error
						? err.message
						: "request failed";
				dispatchVersionCheck({ type: "fail", message });
			}
		},
		[versionCheckLastAtMs],
	);

	useEffect(() => {
		const onFocus = () => {
			void runVersionCheck();
		};

		window.addEventListener("focus", onFocus);
		if (typeof document.hasFocus === "function" && document.hasFocus()) {
			onFocus();
		}

		return () => window.removeEventListener("focus", onFocus);
	}, [runVersionCheck]);

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

	const versionBadges = useMemo(() => {
		const xpVersion =
			clusterInfo.isSuccess && clusterInfo.data?.xp_version
				? clusterInfo.data.xp_version
				: null;
		const repo =
			versionCheck.kind === "update_available" ||
			versionCheck.kind === "up_to_date"
				? versionCheck.repo
				: null;
		const xpHref = xpVersionLinkHref(xpVersion, repo);

		const items: ReactNode[] = [];

		items.push(
			<a
				key="xp-version"
				href={xpHref}
				target="_blank"
				rel="noreferrer"
				className="badge badge-sm gap-2 font-mono badge-ghost hover:bg-base-200 transition-colors"
				title={
					xpVersion
						? `xp version (from /api/cluster/info): ${xpVersion}`
						: "xp version (from /api/cluster/info)"
				}
			>
				<span>xp</span>
				<span className="opacity-80">{xpVersion ?? "…"}</span>
				<Icon name="tabler:external-link" size={14} className="opacity-60" />
			</a>,
		);

		if (versionCheck.kind === "checking") {
			items.push(
				<span
					key="version-check"
					className="badge badge-sm gap-2 font-mono badge-ghost"
					title="Checking latest version…"
				>
					<Icon
						name="tabler:loader-2"
						size={14}
						className="animate-spin opacity-70"
					/>
					<span>checking</span>
				</span>,
			);
		} else if (versionCheck.kind === "update_available") {
			const href = githubReleaseTagUrl(
				versionCheck.latest_tag,
				versionCheck.repo,
			);
			items.push(
				<a
					key="version-check"
					href={href}
					target="_blank"
					rel="noreferrer"
					className="badge badge-sm gap-2 font-mono badge-warning hover:brightness-95 transition"
					title="Update available"
				>
					<Icon name="tabler:download" size={14} className="opacity-80" />
					<span>update:</span>
					<span className="opacity-80">{versionCheck.latest_tag}</span>
					<Icon name="tabler:external-link" size={14} className="opacity-70" />
				</a>,
			);
		} else if (versionCheck.kind === "up_to_date") {
			items.push(
				<span
					key="version-check"
					className="badge badge-sm gap-2 font-mono badge-ghost"
					title={
						versionCheck.comparable
							? `Up to date (checked at ${versionCheck.checked_at})`
							: `Latest is ${versionCheck.latest_tag}, but current version is not comparable`
					}
				>
					<Icon name="tabler:circle-check" size={14} className="opacity-70" />
					<span>{versionCheck.comparable ? "up-to-date" : "unknown"}</span>
				</span>,
			);
		} else if (versionCheck.kind === "check_failed") {
			items.push(
				<button
					key="version-check"
					type="button"
					className="badge badge-sm gap-2 font-mono badge-error hover:brightness-95 transition"
					title={versionCheck.message}
					onClick={() => {
						void runVersionCheck({ force: true });
					}}
				>
					<Icon name="tabler:refresh" size={14} className="opacity-80" />
					<span>retry</span>
				</button>,
			);
		} else {
			items.push(
				<span
					key="version-check"
					className="badge badge-sm gap-2 font-mono badge-ghost"
					title="Focus the page to check updates (1h cooldown)"
				>
					<Icon name="tabler:refresh" size={14} className="opacity-70" />
					<span>update</span>
				</span>,
			);
		}

		return items;
	}, [clusterInfo.data, clusterInfo.isSuccess, runVersionCheck, versionCheck]);

	const effectiveNavGroups =
		navGroups ??
		(navItems
			? [
					{
						title: "NAV",
						items: navItems,
					},
				]
			: []);

	const navContent = (
		<nav
			aria-label="Primary navigation"
			className="rounded-box border border-base-200 bg-base-100 p-4"
		>
			<div className="space-y-6">
				{effectiveNavGroups.map((group) => (
					<div key={group.title} className="space-y-2">
						<p className="px-2 text-xs uppercase tracking-wide opacity-50">
							{group.title}
						</p>
						<ul className="space-y-2">
							{group.items.map((item) => (
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
					</div>
				))}
			</div>
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
								<div className="flex items-center gap-2">{versionBadges}</div>

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

					<div className="grid grid-cols-1 gap-6 lg:grid-cols-[17rem_minmax(0,1fr)]">
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
