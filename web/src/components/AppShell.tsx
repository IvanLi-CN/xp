import { useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useReducer, useState } from "react";

import { Badge } from "@/components/ui/badge";
import {
	CommandDialog,
	CommandEmpty,
	CommandGroup,
	CommandInput,
	CommandItem,
	CommandList,
	CommandShortcut,
} from "@/components/ui/command";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuLabel,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetHeader,
	SheetTitle,
} from "@/components/ui/sheet";
import { fetchAdminAlerts } from "../api/adminAlerts";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { fetchVersionCheck } from "../api/versionCheck";
import { Button } from "./Button";
import { Icon } from "./Icon";
import { useUiPrefs } from "./UiPrefs";
import { VersionBadges } from "./VersionBadges";
import { clearAdminToken, readAdminToken } from "./auth";
import { inputClass } from "./ui-helpers";
import {
	type VersionCheckUiState,
	readVersionCheckLastAtMs,
	reduceVersionCheckUiState,
	shouldAutoCheckVersion,
	writeVersionCheckLastAtMs,
} from "./versionCheckUi";

type AppShellProps = {
	brand: { name: string; subtitle?: string; markSrc?: string };
	navItems?: Array<{ label: string; to: string; icon: string }>;
	navGroups?: Array<{
		title: string;
		items: Array<{ label: string; to: string; icon: string }>;
	}>;
	headerStatus?: ReactNode;
	children?: ReactNode;
};

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
	const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
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
			setCommandPaletteOpen(true);
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

	const navEntries = useMemo(
		() =>
			effectiveNavGroups.flatMap((group) =>
				group.items.map((item) => ({ ...item, group: group.title })),
			),
		[effectiveNavGroups],
	);

	const statusBadges = useMemo(() => {
		const items: ReactNode[] = [];
		const healthOk = health.isSuccess && health.data?.status === "ok";
		items.push(
			<Badge
				key="health"
				variant={healthOk ? "info" : "destructive"}
				size="sm"
				className="gap-2 font-mono"
				title="Backend health"
			>
				<span>Health</span>
				<span className="opacity-80">{healthOk ? "OK" : "error"}</span>
			</Badge>,
		);

		if (clusterInfo.isSuccess) {
			const leaderHost = safeHostFromUrl(clusterInfo.data.leader_api_base_url);
			const leaderValue =
				clusterInfo.data.role === "leader"
					? clusterInfo.data.node_id
					: (leaderHost ?? clusterInfo.data.leader_api_base_url);
			items.push(
				<Badge
					key="leader"
					variant="ghost"
					size="sm"
					className="gap-2 font-mono"
					title="Cluster leader"
				>
					<span>leader:</span>
					<span className="opacity-80">{leaderValue}</span>
				</Badge>,
			);
			items.push(
				<Badge
					key="term"
					variant="ghost"
					size="sm"
					className="gap-2 font-mono"
					title="Cluster term"
				>
					<span>term:</span>
					<span className="opacity-80">{clusterInfo.data.term}</span>
				</Badge>,
			);
		}

		if (adminAlerts.isSuccess) {
			const count = adminAlerts.data.items.length;
			const unreachable = adminAlerts.data.unreachable_nodes.length;
			const partial = adminAlerts.data.partial;
			const variant =
				count > 0 || unreachable > 0 || partial ? "warning" : "ghost";
			items.push(
				<Badge
					key="alerts"
					variant={variant}
					size="sm"
					className="gap-2 font-mono"
					title="Admin alerts"
				>
					<span>alerts:</span>
					<span>{count}</span>
					{unreachable > 0 ? (
						<span className="opacity-80">+{unreachable}n</span>
					) : null}
					{partial ? <span className="opacity-80">partial</span> : null}
				</Badge>,
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

	const statusMenuBadges = headerStatus ?? statusBadges;

	const versionBadges = useMemo(() => {
		const xpVersion =
			clusterInfo.isSuccess && clusterInfo.data?.xp_version
				? clusterInfo.data.xp_version
				: null;
		return (
			<VersionBadges
				xpVersion={xpVersion}
				versionCheck={versionCheck}
				onRetry={() => {
					void runVersionCheck({ force: true });
				}}
			/>
		);
	}, [clusterInfo.data, clusterInfo.isSuccess, runVersionCheck, versionCheck]);

	const navContent = (
		<nav aria-label="Primary navigation" className="xp-panel p-4">
			<div className="space-y-6">
				{effectiveNavGroups.map((group) => (
					<div key={group.title} className="space-y-2">
						<p className="px-2 text-xs uppercase tracking-[0.18em] text-muted-foreground">
							{group.title}
						</p>
						<ul className="space-y-1.5">
							{group.items.map((item) => (
								<li key={item.to}>
									<Link
										to={item.to}
										className="flex items-center gap-3 rounded-xl border border-transparent px-3 py-2 text-sm font-medium text-muted-foreground transition-colors hover:border-border/70 hover:bg-muted/60 hover:text-foreground"
										activeProps={{
											className:
												"border-primary/25 bg-primary/10 text-foreground shadow-sm",
										}}
										onClick={() => setMobileNavOpen(false)}
									>
										<Icon name={item.icon} className="size-5 opacity-80" />
										<span className="truncate">{item.label}</span>
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
		<div className="min-h-screen bg-background">
			<div className="p-4 md:p-5">
				<div className="mx-auto flex max-w-7xl flex-col gap-4">
					<header className="xp-panel px-4 py-3">
						<div className="grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-4">
							<div className="flex min-w-0 items-center gap-3">
								<Button
									variant="ghost"
									size="sm"
									className="lg:hidden"
									aria-label="Open menu"
									onClick={() => setMobileNavOpen(true)}
								>
									<Icon name="tabler:menu-2" ariaLabel="Menu" />
								</Button>
								<Link to="/" className="flex min-w-0 items-center gap-2">
									{brand.markSrc ? (
										<img
											src={brand.markSrc}
											alt=""
											aria-hidden="true"
											className="size-6 shrink-0"
										/>
									) : null}
									<span className="text-lg font-semibold tracking-tight">
										{brand.name}
									</span>
									{brand.subtitle ? (
										<span className="hidden truncate text-sm text-muted-foreground sm:inline">
											{brand.subtitle}
										</span>
									) : null}
								</Link>
							</div>

							<div className="hidden items-center justify-center lg:flex">
								<div className="relative w-full max-w-md">
									<input
										type="text"
										readOnly
										placeholder="Search / command palette"
										className={inputClass(
											prefs.density,
											"cursor-pointer bg-muted/50 pr-20 font-mono",
										)}
										onClick={() => setCommandPaletteOpen(true)}
										onFocus={() => setCommandPaletteOpen(true)}
									/>
									<div className="pointer-events-none absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-1">
										<kbd className="xp-kbd">Ctrl</kbd>
										<kbd className="xp-kbd">K</kbd>
									</div>
								</div>
							</div>

							<div className="flex items-center justify-end gap-2">
								<div className="flex items-center gap-2">{versionBadges}</div>

								<DropdownMenu>
									<DropdownMenuTrigger asChild>
										<Button
											variant="secondary"
											size="sm"
											aria-label="Open status"
										>
											<Icon
												name="tabler:activity-heartbeat"
												ariaLabel="Status"
											/>
											<span className="hidden sm:inline">Status</span>
										</Button>
									</DropdownMenuTrigger>
									<DropdownMenuContent align="end" className="w-80 p-3">
										<DropdownMenuLabel className="px-1 text-xs uppercase tracking-[0.18em] text-muted-foreground">
											Status
										</DropdownMenuLabel>
										<div className="mt-2 flex flex-wrap gap-2">
											{statusMenuBadges}
										</div>
									</DropdownMenuContent>
								</DropdownMenu>

								<DropdownMenu>
									<DropdownMenuTrigger asChild>
										<Button
											variant="secondary"
											size="sm"
											aria-label="Open settings"
										>
											<Icon name="tabler:settings" ariaLabel="Settings" />
											<span className="hidden sm:inline">Settings</span>
										</Button>
									</DropdownMenuTrigger>
									<DropdownMenuContent align="end" className="w-80 p-3">
										<DropdownMenuLabel className="px-1 text-xs uppercase tracking-[0.18em] text-muted-foreground">
											Theme
										</DropdownMenuLabel>
										<div className="mt-2 space-y-3">
											<Select
												value={prefs.theme}
												onValueChange={(next) => {
													if (
														next === "dark" ||
														next === "light" ||
														next === "system"
													) {
														prefs.setTheme(next);
													}
												}}
											>
												<SelectTrigger aria-label="Theme">
													<SelectValue placeholder="Theme" />
												</SelectTrigger>
												<SelectContent>
													<SelectItem value="system">System</SelectItem>
													<SelectItem value="light">Light</SelectItem>
													<SelectItem value="dark">Dark</SelectItem>
												</SelectContent>
											</Select>
											<div className="space-y-1">
												<p className="px-1 text-xs uppercase tracking-[0.18em] text-muted-foreground">
													Density
												</p>
												<Select
													value={prefs.density}
													onValueChange={(next) =>
														prefs.setDensity(
															next === "compact" ? "compact" : "comfortable",
														)
													}
												>
													<SelectTrigger aria-label="Density">
														<SelectValue placeholder="Density" />
													</SelectTrigger>
													<SelectContent>
														<SelectItem value="comfortable">
															Comfortable
														</SelectItem>
														<SelectItem value="compact">Compact</SelectItem>
													</SelectContent>
												</Select>
											</div>
										</div>
										<DropdownMenuSeparator />
										<DropdownMenuItem
											onSelect={() => setCommandPaletteOpen(true)}
										>
											<span>Command palette</span>
											<CommandShortcut>Ctrl/⌘K</CommandShortcut>
										</DropdownMenuItem>
										<DropdownMenuItem
											className="text-destructive focus:text-destructive"
											onSelect={() => {
												clearAdminToken();
												navigate({ to: "/login" });
											}}
										>
											<Icon name="tabler:logout" ariaLabel="Logout" />
											<span>Logout</span>
										</DropdownMenuItem>
									</DropdownMenuContent>
								</DropdownMenu>
							</div>
						</div>
					</header>

					<div className="grid grid-cols-1 gap-6 lg:grid-cols-[17rem_minmax(0,1fr)]">
						<aside className="hidden lg:block">{navContent}</aside>
						<main className="xp-panel p-[var(--xp-page-padding)]">
							{children ?? <Outlet />}
						</main>
					</div>
				</div>
			</div>

			<CommandDialog
				open={commandPaletteOpen}
				onOpenChange={setCommandPaletteOpen}
			>
				<CommandInput placeholder="Search navigation and quick actions" />
				<CommandList>
					<CommandEmpty>No quick actions yet.</CommandEmpty>
					<CommandGroup heading="Navigation">
						{navEntries.map((item) => (
							<CommandItem
								key={item.to}
								onSelect={() => {
									setCommandPaletteOpen(false);
									navigate({ to: item.to as never });
								}}
							>
								<Icon
									name={item.icon}
									ariaLabel={item.label}
									className="size-4"
								/>
								<span>{item.label}</span>
								<CommandShortcut>{item.group}</CommandShortcut>
							</CommandItem>
						))}
					</CommandGroup>
				</CommandList>
			</CommandDialog>

			<Sheet open={mobileNavOpen} onOpenChange={setMobileNavOpen}>
				<SheetContent
					side="left"
					className="flex w-[20rem] flex-col gap-4 p-4 sm:max-w-[20rem]"
				>
					<SheetHeader>
						<SheetTitle>{brand.name}</SheetTitle>
						<SheetDescription>
							{brand.subtitle ?? "Primary navigation"}
						</SheetDescription>
					</SheetHeader>
					{navContent}
				</SheetContent>
			</Sheet>
		</div>
	);
}
