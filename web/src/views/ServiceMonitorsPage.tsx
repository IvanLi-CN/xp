import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";

import {
	type ServiceMonitorSummary,
	fetchAdminServiceMonitors,
	monitorKind,
	monitorTargetLabel,
} from "../api/adminServiceMonitors";
import { useApiCapability } from "../api/useApiCompatibility";
import { Button } from "../components/Button";
import { Icon } from "../components/Icon";
import { PageHeader } from "../components/PageHeader";
import { CapabilityUnavailableState, PageState } from "../components/PageState";
import { ServiceMonitorStatusBadge } from "../components/ServiceMonitorStatusBadge";
import { ServiceMonitorUptimeBar } from "../components/ServiceMonitorUptimeBar";
import { readAdminToken } from "../components/auth";
import { Badge } from "../components/ui/badge";
import { Tabs, TabsList, TabsTrigger } from "../components/ui/tabs";
import { useAppRuntime } from "../offline/appRuntime";

type MonitorFilter = "all" | "attention" | "up";

function percent(value: number | null): string {
	return value === null ? "-" : `${value.toFixed(2)}%`;
}

function age(value: number | null): string {
	if (value === null) return "No check yet";
	const seconds = Math.max(0, Math.floor(Date.now() / 1000) - value);
	if (seconds < 60) return `${seconds}s ago`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
	return `${Math.floor(seconds / 3600)}h ago`;
}

function isAttention(monitor: ServiceMonitorSummary): boolean {
	return monitor.status !== "up" || monitor.stale;
}

function isHealthy(monitor: ServiceMonitorSummary): boolean {
	return monitor.status === "up" && !monitor.stale;
}

function qualityLabel(quality: ServiceMonitorSummary["quality"]): string {
	if (quality === "local_only") return "Local cache";
	return quality === "complete" ? "Complete history" : "Partial history";
}

function historyDetail(monitor: ServiceMonitorSummary): string {
	const rolling = monitor.recent_6h;
	const checks = `${rolling.executed}/${rolling.expected} checks`;
	if (monitor.quality !== "complete") {
		return `${checks} · ${qualityLabel(monitor.quality)}`;
	}
	if (rolling.coverage_percent !== null && rolling.coverage_percent < 100) {
		return `${checks} · ${percent(rolling.coverage_percent)} coverage`;
	}
	return checks;
}

function MonitorRosterRow({ monitor }: { monitor: ServiceMonitorSummary }) {
	const rolling = monitor.recent_6h;
	const latestLatency =
		rolling.latest_latency_ms === null
			? "-"
			: `${rolling.latest_latency_ms} ms`;
	return (
		<Link
			className={[
				"grid gap-x-5 gap-y-3 px-4 py-4 transition-colors",
				"hover:bg-muted/55 focus-visible:bg-muted/55",
				"focus-visible:outline-none sm:px-5",
				"xl:grid-cols-[minmax(12.5rem,0.8fr)_minmax(30rem,2.6fr)_6rem]",
				"xl:items-center",
			].join(" ")}
			to="/monitors/$monitorId"
			params={{ monitorId: monitor.monitor_id }}
		>
			<div className="min-w-0">
				<div className="flex min-w-0 flex-wrap items-center gap-2">
					<p className="min-w-0 truncate font-semibold">{monitor.name}</p>
					<ServiceMonitorStatusBadge
						status={monitor.status}
						stale={monitor.stale}
					/>
				</div>
				<p className="mt-1 truncate text-xs text-muted-foreground">
					{monitorKind(monitor.target).toUpperCase()} ·{" "}
					{monitorTargetLabel(monitor.target)}
				</p>
			</div>
			<div className="min-w-0">
				<div className="flex items-baseline justify-between gap-3">
					<p className="text-xs font-medium text-muted-foreground">
						6h availability
					</p>
					<p className="shrink-0 text-lg font-semibold tabular-nums">
						{percent(rolling.availability_percent)}
					</p>
				</div>
				<ServiceMonitorUptimeBar
					className="mt-2"
					label={`${monitor.name} recent six hour check history`}
					prominent
					slots={rolling.slots}
				/>
				<p className="mt-1.5 text-xs text-muted-foreground">
					{historyDetail(monitor)}
				</p>
			</div>
			<div className="flex items-baseline justify-between gap-3 xl:block xl:text-right">
				<p className="text-xs font-medium text-muted-foreground">Response</p>
				<p className="mt-0.5 text-base font-semibold tabular-nums">
					{latestLatency}
				</p>
				<p className="mt-0.5 text-xs text-muted-foreground">
					{age(rolling.latest_observed_at_unix_seconds)}
				</p>
			</div>
		</Link>
	);
}

export function ServiceMonitorsPage() {
	const adminToken = readAdminToken();
	const runtime = useAppRuntime();
	const capability = useApiCapability("admin.service-monitors");
	const [filter, setFilter] = useState<MonitorFilter>("all");
	const monitorsQuery = useQuery({
		queryKey: ["adminServiceMonitors", adminToken],
		enabled: adminToken.length > 0 && capability.available,
		queryFn: ({ signal }) => fetchAdminServiceMonitors(adminToken, signal),
		refetchInterval: runtime.isOnline ? 30_000 : false,
	});

	if (!adminToken) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to manage service monitors."
			/>
		);
	}
	if (capability.unavailable) {
		return (
			<CapabilityUnavailableState
				title="Service monitoring unavailable"
				reason={capability.reason}
			/>
		);
	}
	if (monitorsQuery.isLoading) {
		return <PageState variant="loading" title="Loading service monitors" />;
	}
	if (monitorsQuery.isError) {
		return (
			<PageState
				variant="error"
				title="Failed to load service monitors"
				error={monitorsQuery.error}
				action={
					<Button variant="secondary" onClick={() => monitorsQuery.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	}

	const monitors = monitorsQuery.data?.items ?? [];
	const attention = monitors.filter(isAttention);
	const healthy = monitors.filter(isHealthy);
	const visibleMonitors =
		filter === "attention" ? attention : filter === "up" ? healthy : monitors;

	return (
		<div className="space-y-6">
			<PageHeader
				title="Service monitoring"
				description="Cluster observer checks and recent six-hour service continuity."
				actions={
					<div className="flex flex-wrap gap-2">
						<Button
							className="w-36"
							variant="secondary"
							loading={monitorsQuery.isFetching}
							iconLeft={<Icon name="tabler:refresh" />}
							onClick={() => monitorsQuery.refetch()}
						>
							Refresh
						</Button>
						<Button asChild className="w-36" disabled={runtime.isReadOnly}>
							<Link to="/monitors/new">
								<Icon name="tabler:plus" />
								New monitor
							</Link>
						</Button>
					</div>
				}
			/>
			{monitors.length === 0 ? (
				<PageState
					variant="empty"
					title="No service monitors"
					description="Add a public target to begin collecting observations."
					action={
						<Button asChild>
							<Link to="/monitors/new">Create monitor</Link>
						</Button>
					}
				/>
			) : (
				<>
					{attention.length > 0 ? (
						<section
							aria-labelledby="monitor-attention-heading"
							className="rounded-2xl border border-warning/40 bg-warning/8 px-4 py-3 sm:px-5"
						>
							<div className="flex flex-wrap items-center gap-x-3 gap-y-2">
								<div className="flex items-center gap-2">
									<Icon name="tabler:alert-triangle" className="text-warning" />
									<p id="monitor-attention-heading" className="font-semibold">
										Needs attention
									</p>
									<Badge variant="warning" size="sm">
										{attention.length}
									</Badge>
								</div>
								<div className="flex min-w-0 flex-wrap gap-x-3 gap-y-1 text-sm text-muted-foreground">
									{attention.map((monitor) => (
										<Link
											key={monitor.monitor_id}
											className="truncate text-foreground underline-offset-4 hover:underline"
											to="/monitors/$monitorId"
											params={{ monitorId: monitor.monitor_id }}
										>
											{monitor.name}
										</Link>
									))}
								</div>
							</div>
						</section>
					) : null}
					<section className="overflow-hidden rounded-2xl border border-border/70 bg-card shadow-sm">
						<div
							className={[
								"flex flex-wrap items-center justify-between gap-3",
								"border-b border-border/70 px-4 py-3 sm:px-5",
							].join(" ")}
						>
							<div>
								<h2 className="font-semibold">Monitor roster</h2>
								<p className="mt-0.5 text-xs text-muted-foreground">
									Six-hour continuity is grouped into five-minute buckets from
									the local observer cache.
								</p>
							</div>
							<Tabs
								value={filter}
								onValueChange={(value) => setFilter(value as MonitorFilter)}
							>
								<TabsList aria-label="Monitor filters" className="h-9">
									<TabsTrigger value="all" className="h-7 px-2.5 text-xs">
										All {monitors.length}
									</TabsTrigger>
									<TabsTrigger value="attention" className="h-7 px-2.5 text-xs">
										Attention {attention.length}
									</TabsTrigger>
									<TabsTrigger value="up" className="h-7 px-2.5 text-xs">
										Up {healthy.length}
									</TabsTrigger>
								</TabsList>
							</Tabs>
						</div>
						<div
							className={[
								"hidden gap-x-5 border-b border-border/70 px-5 py-2",
								"text-xs font-medium text-muted-foreground xl:grid",
								"xl:grid-cols-[minmax(12.5rem,0.8fr)_minmax(30rem,2.6fr)_6rem]",
							].join(" ")}
						>
							<span>Monitor</span>
							<span>6h availability and five-minute continuity</span>
							<span className="text-right">Response</span>
						</div>
						<div
							aria-label="Service monitor roster"
							className="divide-y divide-border/70"
						>
							{visibleMonitors.map((monitor) => (
								<MonitorRosterRow key={monitor.monitor_id} monitor={monitor} />
							))}
							{visibleMonitors.length === 0 ? (
								<p className="px-5 py-10 text-center text-sm text-muted-foreground">
									No monitors match this filter.
								</p>
							) : null}
						</div>
					</section>
				</>
			)}
		</div>
	);
}
