import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
	Link,
	Outlet,
	useNavigate,
	useRouterState,
} from "@tanstack/react-router";
import type { ReactNode } from "react";
import {
	useCallback,
	useEffect,
	useMemo,
	useReducer,
	useRef,
	useState,
} from "react";

import { fetchAdminNodes } from "@/api/adminNodes";
import { startAdminStatusEvents } from "@/api/adminStatusEvents";
import { usePrimaryBackend } from "@/backend/PrimaryBackendProvider";
import { hydratePrimaryBackendProfile } from "@/backend/primaryBackend";
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
import {
	fetchAdminUpgradeStatus,
	startAdminUpgrade,
} from "../api/adminUpgrade";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import {
	ApiCompatibilityProvider,
	useApiCompatibility,
} from "../api/useApiCompatibility";
import { fetchVersionCheck } from "../api/versionCheck";
import { useAppRuntime } from "../offline/appRuntime";
import { requestServiceWorkerUpdateCheck } from "../offline/serviceWorkerUpdates";
import { classifyUpgradeStartError } from "../offline/upgradeObservation";
import { shouldRefreshAfterUpgradeTransition } from "../offline/upgradeRefreshPolicy";
import { useUpgradeObservation } from "../offline/useUpgradeObservation";
import {
	type AppNavigationGroup,
	AppResourceNavigation,
} from "./AppResourceNavigation";
import { Button } from "./Button";
import { Icon } from "./Icon";
import { useObjectNavigationGuard } from "./ObjectNavigationGuard";
import { PrimaryBackendSwitcher } from "./PrimaryBackendSwitcher";
import { ReadStateIndicator } from "./ReadStateIndicator";
import { useUiPrefs } from "./UiPrefs";
import { VersionIndicator } from "./VersionIndicator";
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
	navGroups?: AppNavigationGroup[];
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

function formatApiError(err: unknown): string {
	if (isBackendApiError(err)) {
		return err.code
			? `${err.status} ${err.code}: ${err.message}`
			: `${err.status}: ${err.message}`;
	}
	return err instanceof Error ? err.message : "request failed";
}

export function AppShell({
	brand,
	navItems,
	navGroups,
	headerStatus,
	children,
}: AppShellProps) {
	const navigate = useNavigate();
	const pathname = useRouterState({
		select: (state) => state.location.pathname,
	});
	const queryClient = useQueryClient();
	const runtime = useAppRuntime();
	const prefs = useUiPrefs();
	const primaryBackend = usePrimaryBackend();
	const [adminToken] = useState(() => readAdminToken());
	const apiCompatibility = useApiCompatibility(adminToken, runtime.isOnline);
	const { requestNavigation } = useObjectNavigationGuard();
	const compatibility = apiCompatibility.data ?? null;
	const compatibilityError =
		apiCompatibility.isError && compatibility === null
			? formatApiError(apiCompatibility.error)
			: !runtime.isOnline && compatibility === null
				? "API compatibility is unavailable while offline."
				: null;
	const alertsCapabilityAvailable =
		apiCompatibility.data?.kind === "compatible" &&
		apiCompatibility.data.isFeatureAvailable("admin.alerts");
	const upgradeCapabilityAvailable =
		apiCompatibility.data?.kind === "compatible" &&
		apiCompatibility.data.isFeatureAvailable("admin.upgrade");
	const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
	const [mobileNavOpen, setMobileNavOpen] = useState(false);
	const [versionCheck, dispatchVersionCheck] = useReducer(
		reduceVersionCheckUiState,
		{ kind: "idle" } satisfies VersionCheckUiState,
	);
	const [versionCheckLastAtMs, setVersionCheckLastAtMs] = useState<
		number | null
	>(() => readVersionCheckLastAtMs());
	const [statusStream, setStatusStream] = useState<{
		connected: boolean;
		lastEventAtMs: number | null;
		error: string | null;
	}>({
		connected: false,
		lastEventAtMs: null,
		error: null,
	});
	const lastUpgradeStateRef = useRef<string | null>(null);
	const lastMeshRevisionRef = useRef<number | null>(null);
	const upgradeObservation = useUpgradeObservation();

	const health = useQuery({
		queryKey: ["health"],
		queryFn: ({ signal }) => fetchHealth(signal),
	});

	const clusterInfo = useQuery({
		queryKey: ["clusterInfo"],
		queryFn: ({ signal }) => fetchClusterInfo(signal),
	});

	const backendNodes = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled:
			adminToken.length > 0 &&
			Boolean(clusterInfo.data?.cluster_id) &&
			clusterInfo.data?.cluster_id === primaryBackend.clusterId,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const refreshBackendCandidates = useCallback(() => {
		if (!adminToken || !primaryBackend.clusterId) return;
		void backendNodes.refetch();
	}, [adminToken, backendNodes.refetch, primaryBackend.clusterId]);

	useEffect(() => {
		const clusterId = clusterInfo.data?.cluster_id;
		if (!clusterId) return;
		hydratePrimaryBackendProfile(
			clusterId,
			backendNodes.isSuccess ? backendNodes.data.items : undefined,
		);
	}, [
		clusterInfo.data?.cluster_id,
		backendNodes.data?.items,
		backendNodes.isSuccess,
	]);

	useEffect(() => {
		if (primaryBackend.state !== "unreachable") return;
		refreshBackendCandidates();
	}, [primaryBackend.state, refreshBackendCandidates]);

	const previousBackendGeneration = useRef(primaryBackend.generation);
	useEffect(() => {
		if (previousBackendGeneration.current === primaryBackend.generation) return;
		previousBackendGeneration.current = primaryBackend.generation;
		void (async () => {
			await queryClient.cancelQueries();
			await queryClient.invalidateQueries();
		})();
	}, [primaryBackend.generation, queryClient]);

	const adminAlerts = useQuery({
		queryKey: ["adminAlerts", adminToken],
		enabled: adminToken.length > 0 && alertsCapabilityAvailable,
		queryFn: ({ signal }) => fetchAdminAlerts(adminToken, signal),
	});

	const adminUpgradeStatus = useQuery({
		queryKey: ["adminUpgradeStatus", adminToken],
		enabled: adminToken.length > 0 && upgradeCapabilityAvailable,
		queryFn: ({ signal }) => fetchAdminUpgradeStatus(adminToken, signal),
		refetchInterval: (query) => {
			const state = query.state.data?.status.state;
			return upgradeObservation.isObserving ||
				state === "running" ||
				state === "restarting"
				? 2500
				: false;
		},
	});

	const adminUpgradeStart = useMutation({
		mutationFn: async (payload: { targetTag: string }) => {
			if (!upgradeCapabilityAvailable) {
				throw new Error("The admin upgrade capability is unavailable.");
			}
			return startAdminUpgrade(adminToken, {
				target_tag: payload.targetTag,
			});
		},
		onSuccess: (data) => {
			queryClient.setQueryData(["adminUpgradeStatus", adminToken], data);
			void clusterInfo.refetch();
			void runVersionCheck({ force: true });
		},
		onError: (error) => {
			const disposition = classifyUpgradeStartError(error);
			if (disposition === "failed") {
				upgradeObservation.clear();
				return;
			}
			void adminUpgradeStatus.refetch().then((result) => {
				if (
					disposition === "observe_existing" &&
					result.isSuccess &&
					result.data
				) {
					upgradeObservation.observeExistingStatus(result.data.status);
				}
			});
		},
	});
	const upgradeStatusData = adminUpgradeStatus.data;
	const dismissUpgradeResult = useCallback(() => {
		adminUpgradeStart.reset();
		upgradeObservation.clear();
	}, [adminUpgradeStart.reset, upgradeObservation.clear]);

	useEffect(() => {
		if (!upgradeStatusData) return;
		upgradeObservation.observeStatus(upgradeStatusData.status);
	}, [upgradeObservation.observeStatus, upgradeStatusData]);

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
			if (!runtime.isOnline) return;
			const nowMs = Date.now();
			const canRun =
				force ||
				shouldAutoCheckVersion({ nowMs, lastAtMs: versionCheckLastAtMs });
			if (!canRun) return;

			writeVersionCheckLastAtMs(nowMs);
			setVersionCheckLastAtMs(nowMs);
			dispatchVersionCheck({ type: "start" });

			try {
				const data = await fetchVersionCheck({
					refresh: force,
					adminToken: force ? adminToken : undefined,
				});
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
		[adminToken, runtime.isOnline, versionCheckLastAtMs],
	);

	useEffect(() => {
		// The stream lifetime is bound to the selected backend generation.
		void primaryBackend.generation;
		if (adminToken.length === 0) {
			setStatusStream({ connected: false, lastEventAtMs: null, error: null });
			return;
		}

		const compatibility = apiCompatibility.data;
		const statusEventsAvailable =
			runtime.isOnline &&
			compatibility?.kind === "compatible" &&
			compatibility.isFeatureAvailable("admin.status-events");
		if (!statusEventsAvailable) {
			setStatusStream({
				connected: false,
				lastEventAtMs: null,
				error:
					compatibility?.kind === "compatible"
						? compatibility.degradationFor("admin.status-events")
						: (compatibility?.reason ?? "API compatibility is unavailable"),
			});
			return;
		}

		const handle = startAdminStatusEvents({
			adminToken,
			onOpen: () => {
				setStatusStream((prev) => ({ ...prev, connected: true, error: null }));
			},
			onClose: () => {
				setStatusStream((prev) => ({ ...prev, connected: false }));
			},
			onError: (error) => {
				setStatusStream((prev) => ({
					...prev,
					connected: false,
					error: error instanceof Error ? error.message : String(error),
				}));
			},
			onMessage: (message) => {
				if (message.type === "snapshot_error") {
					setStatusStream((prev) => ({
						...prev,
						error: message.data.message,
					}));
					return;
				}

				if (message.type === "snapshot") {
					const lastEventAtMs = Date.parse(message.data.emitted_at);
					queryClient.setQueryData(["health"], message.data.health);
					queryClient.setQueryData(["clusterInfo"], message.data.cluster_info);
					queryClient.setQueryData(
						["adminAlerts", adminToken],
						message.data.alerts,
					);
					queryClient.setQueryData(
						["adminNodesRuntime", adminToken],
						message.data.nodes_runtime,
					);
					queryClient.setQueryData(
						["adminUpgradeStatus", adminToken],
						message.data.upgrade,
					);
					if (lastMeshRevisionRef.current !== message.data.mesh_revision) {
						lastMeshRevisionRef.current = message.data.mesh_revision;
						void queryClient.invalidateQueries({
							queryKey: ["adminMeshStatus", adminToken],
						});
					}
					setStatusStream({
						connected: true,
						lastEventAtMs: Number.isFinite(lastEventAtMs)
							? lastEventAtMs
							: Date.now(),
						error: null,
					});
				}
			},
		});

		return () => handle.close();
	}, [
		adminToken,
		apiCompatibility.data,
		primaryBackend.generation,
		queryClient,
		runtime.isOnline,
	]);

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

	useEffect(() => {
		const nextState = adminUpgradeStatus.data?.status.state ?? null;
		const prevState = lastUpgradeStateRef.current;
		lastUpgradeStateRef.current = nextState;

		if (!shouldRefreshAfterUpgradeTransition(prevState, nextState)) {
			return;
		}

		void clusterInfo.refetch();
		void health.refetch();
		void runVersionCheck({ force: true });
		if (nextState === "succeeded") {
			void requestServiceWorkerUpdateCheck();
		}
	}, [
		adminUpgradeStatus.data?.status.state,
		clusterInfo,
		health,
		runVersionCheck,
	]);

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

	const navigateLink = useCallback(
		(href: string) => {
			setMobileNavOpen(false);
			void navigate({ to: href as never });
		},
		[navigate],
	);

	const navigateResource = useCallback(
		(href: string) => {
			requestNavigation(() => {
				navigateLink(href);
			});
		},
		[navigateLink, requestNavigation],
	);

	const navEntries = useMemo(
		() =>
			effectiveNavGroups.flatMap((group) =>
				group.items.map((item) => ({ ...item, group: group.title })),
			),
		[effectiveNavGroups],
	);

	const statusBadges = useMemo(() => {
		const items: ReactNode[] = [];
		const compatibilityUnavailable =
			apiCompatibility.data?.kind === "incompatible" ||
			(apiCompatibility.data?.kind === "compatible" &&
				!apiCompatibility.data.isFeatureAvailable("admin.status-events"));
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

		items.push(
			<Badge
				key="realtime"
				variant={
					!runtime.isOnline
						? "warning"
						: statusStream.connected
							? "success"
							: "outline"
				}
				size="sm"
				className="gap-2 font-mono"
				title="Realtime status stream"
			>
				<span>realtime:</span>
				<span>
					{!runtime.isOnline
						? "offline"
						: compatibilityUnavailable
							? "unavailable"
							: statusStream.connected
								? "live"
								: "reconnecting"}
				</span>
			</Badge>,
		);

		return items;
	}, [
		adminAlerts.data,
		adminAlerts.isSuccess,
		clusterInfo.data,
		clusterInfo.isSuccess,
		health.data,
		health.isSuccess,
		apiCompatibility.data,
		runtime.isOnline,
		statusStream.connected,
	]);

	const statusMenuBadges = headerStatus ?? statusBadges;

	const versionIndicator = useMemo(() => {
		const xpVersion =
			clusterInfo.isSuccess && clusterInfo.data?.xp_version
				? clusterInfo.data.xp_version
				: null;
		const upgradeError = upgradeObservation.observation
			? null
			: adminUpgradeStart.isError && adminUpgradeStart.error
				? formatApiError(adminUpgradeStart.error)
				: adminUpgradeStatus.isError && adminUpgradeStatus.error
					? formatApiError(adminUpgradeStatus.error)
					: null;
		return (
			<VersionIndicator
				xpVersion={xpVersion}
				versionCheck={versionCheck}
				upgradeStatus={adminUpgradeStatus.data ?? null}
				upgradeStatusError={upgradeError}
				upgradeStatusLoading={adminUpgradeStatus.isFetching}
				upgradeStarting={adminUpgradeStart.isPending}
				upgradeObservation={upgradeObservation.observation}
				onDismissUpgradeResult={dismissUpgradeResult}
				onRetryVersionCheck={() => {
					void runVersionCheck({ force: true });
				}}
				onRefreshUpgradeStatus={() => {
					void adminUpgradeStatus.refetch().then((result) => {
						if (result.isSuccess && result.data) {
							upgradeObservation.refreshTimedOutStatus(result.data.status);
						}
					});
				}}
				onStartUpgrade={(targetTag) => {
					if (!upgradeCapabilityAvailable) return;
					upgradeObservation.begin(targetTag);
					adminUpgradeStart.mutate({ targetTag });
				}}
			/>
		);
	}, [
		adminUpgradeStart,
		adminUpgradeStatus,
		clusterInfo,
		dismissUpgradeResult,
		upgradeCapabilityAvailable,
		runVersionCheck,
		upgradeObservation,
		versionCheck,
	]);

	const globalReadStateIndicator = runtime.isReadOnly ? (
		<ReadStateIndicator
			tone="warning"
			label="Offline cached"
			title={
				runtime.readOnlyReason ??
				"Offline read-only mode is active. Cached data remains available."
			}
		/>
	) : null;
	const serviceMonitorWorkspace = pathname.startsWith("/monitors");

	const navContent = (
		<AppResourceNavigation
			adminToken={adminToken}
			compatibility={compatibility}
			compatibilityError={compatibilityError}
			compatibilityPending={apiCompatibility.isFetching}
			groups={effectiveNavGroups}
			localNodeId={clusterInfo.data?.node_id ?? null}
			pathname={pathname}
			onNavigate={navigateLink}
			onResourceNavigate={navigateResource}
			onRetryCompatibility={() => void apiCompatibility.refetch()}
		/>
	);

	return (
		<div className="min-h-screen bg-background">
			<div className="p-1.5 sm:p-4 md:p-5">
				<div
					className={`mx-auto flex ${
						serviceMonitorWorkspace ? "max-w-[90rem]" : "max-w-7xl"
					} flex-col gap-2.5 sm:gap-4`}
				>
					<header className="xp-panel px-2 py-2 sm:px-4 sm:py-3">
						<div className="grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-1 sm:gap-4">
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
									<button
										type="button"
										aria-label="Open command palette"
										aria-haspopup="dialog"
										aria-expanded={commandPaletteOpen}
										className={inputClass(
											prefs.density,
											"cursor-pointer justify-start bg-muted/50 pr-20 text-left font-mono text-muted-foreground hover:bg-muted",
										)}
										onClick={() => setCommandPaletteOpen(true)}
									>
										<span>Search / command palette</span>
									</button>
									<div className="pointer-events-none absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-1">
										<kbd className="xp-kbd">Ctrl</kbd>
										<kbd className="xp-kbd">K</kbd>
									</div>
								</div>
							</div>

							<div className="flex items-center justify-end gap-1 sm:gap-2">
								<div className="flex items-center gap-1 sm:gap-2">
									<span className="hidden items-center sm:inline-flex">
										{globalReadStateIndicator}
									</span>
									<PrimaryBackendSwitcher
										adminToken={adminToken}
										clusterId={
											clusterInfo.data?.cluster_id ?? primaryBackend.clusterId
										}
										onOpened={refreshBackendCandidates}
									/>
									{versionIndicator}
								</div>

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
							<ApiCompatibilityProvider value={apiCompatibility.data ?? null}>
								{children ?? <Outlet />}
							</ApiCompatibilityProvider>
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
