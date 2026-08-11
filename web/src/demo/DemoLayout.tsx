import {
	Link,
	Outlet,
	useNavigate,
	useRouterState,
} from "@tanstack/react-router";
import type { ReactNode } from "react";
import { useMemo, useState } from "react";

import { useUiPrefs } from "@/components/UiPrefs";
import { Badge } from "@/components/ui/badge";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";

import { Button } from "../components/Button";
import { Icon } from "../components/Icon";
import {
	ObjectNavigationGuardProvider,
	useObjectNavigationGuard,
} from "../components/ObjectNavigationGuard";
import {
	ResourceNavigation,
	type ResourceNavigationGroup,
} from "../components/ResourceNavigation";
import { ToastProvider } from "../components/Toast";
import {
	Sheet,
	SheetContent,
	SheetHeader,
	SheetTitle,
} from "../components/ui/sheet";
import { DEMO_SCENARIOS, getScenario } from "./fixtures";
import { DemoProvider, useDemo } from "./store";

const navGroups = [
	{
		title: "Demo",
		items: [
			{ label: "Dashboard", to: "/demo", icon: "tabler:layout-dashboard" },
			{
				label: "System status",
				to: "/demo/system-status",
				icon: "tabler:activity-heartbeat",
			},
			{ label: "Nodes", to: "/demo/nodes", icon: "tabler:server" },
			{ label: "Endpoints", to: "/demo/endpoints", icon: "tabler:plug" },
			{ label: "Users", to: "/demo/users", icon: "tabler:users" },
		],
	},
	{
		title: "Settings",
		items: [
			{ label: "Quota policy", to: "/demo/quota-policy", icon: "tabler:gauge" },
			{
				label: "Service config",
				to: "/demo/service-config",
				icon: "tabler:settings",
			},
			{ label: "Tools", to: "/demo/tools", icon: "tabler:tool" },
		],
	},
	{
		title: "Playback",
		items: [
			{ label: "Scenarios", to: "/demo/scenarios", icon: "tabler:route" },
		],
	},
] as const;

const themeOptions = [
	{ value: "system", label: "System", icon: "tabler:device-desktop" },
	{ value: "light", label: "Light", icon: "tabler:sun" },
	{ value: "dark", label: "Dark", icon: "tabler:moon" },
] as const;

const densityOptions = [
	{ value: "comfortable", label: "Comfortable", icon: "tabler:layout-grid" },
	{ value: "compact", label: "Compact", icon: "tabler:layout-list" },
] as const;

export function DemoLoginRoute() {
	return (
		<DemoProvider>
			<ToastProvider>
				<Outlet />
			</ToastProvider>
		</DemoProvider>
	);
}

export function DemoAppRoute() {
	return (
		<DemoProvider>
			<ObjectNavigationGuardProvider>
				<ToastProvider>
					<DemoShell>
						<Outlet />
					</DemoShell>
				</ToastProvider>
			</ObjectNavigationGuardProvider>
		</DemoProvider>
	);
}

function DemoShell({ children }: { children: ReactNode }) {
	const navigate = useNavigate();
	const pathname = useRouterState({
		select: (router) => router.location.pathname,
	});
	const { requestNavigation } = useObjectNavigationGuard();
	const { state, logout, resetScenario } = useDemo();
	const prefs = useUiPrefs();
	const [mobileOpen, setMobileOpen] = useState(false);
	const scenario = getScenario(state.scenarioId);
	const degradedNodes = state.nodes.filter((node) => node.status !== "healthy");
	const activeAlerts =
		degradedNodes.length +
		state.endpoints.filter((endpoint) => endpoint.status !== "serving").length +
		state.users.filter((user) => user.status === "quota_limited").length;

	const statusBadges = useMemo(
		() => (
			<>
				<Badge variant={activeAlerts > 0 ? "warning" : "success"} size="sm">
					alerts: {activeAlerts}
				</Badge>
				<Badge variant="ghost" size="sm" className="font-mono">
					leader:{" "}
					{state.nodes.find((node) => node.role === "leader")?.name ?? "-"}
				</Badge>
				<Badge variant="ghost" size="sm" className="font-mono">
					role: {state.session?.role ?? "viewer"}
				</Badge>
			</>
		),
		[activeAlerts, state.nodes, state.session?.role],
	);

	const settingsMenu = (
		<Popover>
			<PopoverTrigger asChild>
				<Button variant="secondary" size="sm" aria-label="Open demo settings">
					<Icon name="tabler:settings" ariaLabel="Settings" />
					<span className="hidden sm:inline">Settings</span>
				</Button>
			</PopoverTrigger>
			<PopoverContent align="end" className="w-[19rem] space-y-4 p-4">
				<div className="space-y-2">
					<p className="px-1 text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Theme
					</p>
					<div
						role="radiogroup"
						aria-label="Demo theme"
						className="grid grid-cols-3 gap-1 rounded-xl bg-muted p-1"
					>
						{themeOptions.map((option) => (
							<label
								key={option.value}
								className={cn(
									"relative flex min-h-10 cursor-pointer items-center justify-center gap-1.5 rounded-lg px-2 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground has-[:focus-visible]:outline-none has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring",
									prefs.theme === option.value &&
										"bg-background text-foreground shadow-sm",
								)}
							>
								<input
									type="radio"
									name="demo-theme"
									value={option.value}
									checked={prefs.theme === option.value}
									onChange={() => prefs.setTheme(option.value)}
									className="absolute inset-0 cursor-pointer opacity-0"
								/>
								<Icon
									name={option.icon}
									className="pointer-events-none size-4"
								/>
								<span className="pointer-events-none">{option.label}</span>
							</label>
						))}
					</div>
				</div>

				<div className="space-y-2">
					<p className="px-1 text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Density
					</p>
					<div
						role="radiogroup"
						aria-label="Demo density"
						className="grid grid-cols-2 gap-1 rounded-xl bg-muted p-1"
					>
						{densityOptions.map((option) => (
							<label
								key={option.value}
								className={cn(
									"relative flex min-h-10 cursor-pointer items-center justify-center gap-1.5 rounded-lg px-2 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground has-[:focus-visible]:outline-none has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring",
									prefs.density === option.value &&
										"bg-background text-foreground shadow-sm",
								)}
							>
								<input
									type="radio"
									name="demo-density"
									value={option.value}
									checked={prefs.density === option.value}
									onChange={() => prefs.setDensity(option.value)}
									className="absolute inset-0 cursor-pointer opacity-0"
								/>
								<Icon
									name={option.icon}
									className="pointer-events-none size-4"
								/>
								<span className="pointer-events-none">{option.label}</span>
							</label>
						))}
					</div>
				</div>
				<p className="px-1 text-xs leading-relaxed text-muted-foreground">
					Saved to this browser for repeatable demo reviews.
				</p>
			</PopoverContent>
		</Popover>
	);

	const resourceNavigationGroups = useMemo<ResourceNavigationGroup[]>(
		() =>
			navGroups.map((group) => ({
				title: group.title,
				items: group.items.map((item) => {
					const resourceId =
						item.to === "/demo/nodes"
							? "nodes"
							: item.to === "/demo/endpoints"
								? "endpoints"
								: item.to === "/demo/users"
									? "users"
									: null;
					const children =
						resourceId === "nodes"
							? state.nodes.map((node) => {
									const isLocalNode = node.id === state.localNodeId;
									return {
										id: node.id,
										label: node.name || "Unnamed node",
										href: `/demo/nodes/${encodeURIComponent(node.id)}`,
										ariaLabel: `${isLocalNode ? "Current hosting node" : "Node"} ${node.name || "unnamed"} (${node.id})`,
										leadingIcon: {
											name: isLocalNode
												? "tabler:server-bolt"
												: "tabler:server-2",
											tone: isLocalNode ? "primary" : "muted",
										} as const,
									};
								})
							: resourceId === "endpoints"
								? state.endpoints.map((endpoint) => ({
										id: endpoint.id,
										label: endpoint.name || "Untitled endpoint",
										href: `/demo/endpoints/${encodeURIComponent(endpoint.id)}`,
										ariaLabel: `Endpoint ${endpoint.name || "untitled"} (${endpoint.id})`,
										leadingIcon: {
											name: "tabler:link",
											tone: "muted",
										} as const,
									}))
								: resourceId === "users"
									? state.users.map((user) => ({
											id: user.id,
											label: user.displayName || "Unnamed user",
											href: `/demo/users/${encodeURIComponent(user.id)}`,
											ariaLabel: `User ${user.displayName || "unnamed"} (${user.id})`,
											leadingIcon: {
												name: "tabler:user-circle",
												tone: "muted",
											} as const,
										}))
									: undefined;
					return {
						id: resourceId ?? item.to,
						label: item.label,
						href: item.to,
						icon: item.icon,
						...(resourceId ? { children } : {}),
					};
				}),
			})),
		[state.endpoints, state.localNodeId, state.nodes, state.users],
	);

	const nav = (
		<ResourceNavigation
			ariaLabel="Demo navigation"
			groups={resourceNavigationGroups}
			pathname={pathname}
			onNavigate={(href) => {
				setMobileOpen(false);
				void navigate({ to: href as never });
			}}
			onResourceNavigate={(href) => {
				requestNavigation(() => {
					setMobileOpen(false);
					void navigate({ to: href as never });
				});
			}}
		/>
	);

	return (
		<div className="min-h-screen bg-background">
			<div className="p-4 md:p-5">
				<div className="mx-auto flex max-w-7xl flex-col gap-4">
					<header className="xp-panel px-4 py-3">
						<div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-center">
							<div className="flex min-w-0 items-center gap-3">
								<Button
									variant="ghost"
									size="sm"
									className="lg:hidden"
									aria-label="Open demo menu"
									onClick={() => setMobileOpen(true)}
								>
									<Icon name="tabler:menu-2" ariaLabel="Menu" />
								</Button>
								<Link to="/demo" className="flex min-w-0 items-center gap-2">
									<img
										src="/xp-mark.png"
										alt=""
										aria-hidden="true"
										className="size-7 shrink-0"
									/>
									<div className="min-w-0">
										<p className="text-lg font-semibold leading-tight">
											xp demo
										</p>
										<p className="truncate text-xs text-muted-foreground">
											{scenario.name}: {scenario.description}
										</p>
									</div>
								</Link>
							</div>

							<div className="flex flex-wrap items-center gap-2 lg:justify-end">
								<div className="flex flex-wrap items-center gap-2">
									{statusBadges}
								</div>
								<Select
									value={state.scenarioId}
									onValueChange={(value) => {
										resetScenario(value as typeof state.scenarioId);
										navigate({ to: "/demo" });
									}}
								>
									<SelectTrigger
										className="w-[12.5rem]"
										aria-label="Demo scenario"
									>
										<SelectValue />
									</SelectTrigger>
									<SelectContent>
										{DEMO_SCENARIOS.map((item) => (
											<SelectItem key={item.id} value={item.id}>
												{item.name}
											</SelectItem>
										))}
									</SelectContent>
								</Select>
								{settingsMenu}
								<Button
									variant="secondary"
									size="sm"
									onClick={() => {
										logout();
										navigate({ to: "/demo/login" });
									}}
								>
									Logout
								</Button>
							</div>
						</div>
					</header>

					<div className="grid grid-cols-1 gap-6 lg:grid-cols-[17rem_minmax(0,1fr)]">
						<aside className="hidden lg:block">{nav}</aside>
						<main className="xp-panel p-[var(--xp-page-padding)]">
							{children}
						</main>
					</div>
				</div>
			</div>

			<Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
				<SheetContent side="left" className="w-[20rem] p-4 sm:max-w-[20rem]">
					<SheetHeader>
						<SheetTitle>xp demo</SheetTitle>
					</SheetHeader>
					<div className="mt-4">{nav}</div>
				</SheetContent>
			</Sheet>
		</div>
	);
}
