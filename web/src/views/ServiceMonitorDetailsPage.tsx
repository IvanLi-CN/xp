import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useState } from "react";

import {
	type ServiceMonitorHistoryResponse,
	type ServiceMonitorStatus,
	deleteAdminServiceMonitor,
	fetchAdminServiceMonitor,
	fetchAdminServiceMonitorHistory,
	fetchAdminServiceMonitorStatus,
	monitorKind,
	monitorTargetLabel,
	patchAdminServiceMonitor,
	runAdminServiceMonitor,
} from "../api/adminServiceMonitors";
import { isBackendApiError } from "../api/backendError";
import { useApiCapability } from "../api/useApiCompatibility";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { DataTable, TableCell } from "../components/DataTable";
import { PageHeader } from "../components/PageHeader";
import { CapabilityUnavailableState, PageState } from "../components/PageState";
import { ReadStateBanner } from "../components/ReadStateBanner";
import { ServiceMonitorHistoryChart } from "../components/ServiceMonitorHistoryChart";
import { ServiceMonitorStatusBadge } from "../components/ServiceMonitorStatusBadge";
import { ServiceMonitorUptimeBar } from "../components/ServiceMonitorUptimeBar";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { Badge } from "../components/ui/badge";
import {
	Card,
	CardContent,
	CardHeader,
	CardTitle,
} from "../components/ui/card";
import { TableRow } from "../components/ui/table";
import { useAppRuntime } from "../offline/appRuntime";

function errorMessage(error: unknown): string {
	if (isBackendApiError(error)) return `${error.code}: ${error.message}`;
	return error instanceof Error ? error.message : String(error);
}

function percent(value: number | null | undefined): string {
	return value === null || value === undefined ? "-" : `${value.toFixed(2)}%`;
}

function age(value: number | null | undefined): string {
	if (value === null || value === undefined) return "No observation";
	if (value < 60) return `${value}s ago`;
	if (value < 3600) return `${Math.floor(value / 60)}m ago`;
	return `${Math.floor(value / 3600)}h ago`;
}

function time(value: number | null | undefined): string {
	return value === null || value === undefined
		? "-"
		: new Date(value * 1000)
				.toISOString()
				.replace("T", " ")
				.replace(".000Z", " UTC");
}

function qualityMessage(
	quality: "complete" | "partial" | "local_only",
): string | null {
	if (quality === "complete") return null;
	return quality === "partial"
		? [
				"Repository history has known gaps or a bounded response.",
				"Coverage and availability only reflect received data.",
			].join(" ")
		: "Showing node-local observations. Repository history is not ready for a complete cluster view.";
}

function Metric({
	label,
	value,
	detail,
}: { label: string; value: string; detail?: string }) {
	return (
		<div className="border-l border-border/70 px-4 first:border-l-0">
			<p className="text-xs font-medium text-muted-foreground">{label}</p>
			<p className="mt-1 text-xl font-semibold tabular-nums">{value}</p>
			{detail ? (
				<p className="mt-1 text-xs text-muted-foreground">{detail}</p>
			) : null}
		</div>
	);
}

function historyTimelineSlots(
	points: ServiceMonitorHistoryResponse["points"],
): ServiceMonitorStatus[] {
	if (points.length === 0) return Array.from({ length: 24 }, () => "unknown");
	return Array.from({ length: 24 }, (_, index) => {
		const start = Math.floor((index * points.length) / 24);
		const end = Math.floor(((index + 1) * points.length) / 24);
		const rollup = points.slice(start, end).reduce(
			(total, point) => ({
				successes: total.successes + point.rollup.successes,
				failures: total.failures + point.rollup.failures,
				suspended: total.suspended + point.rollup.suspended,
			}),
			{ successes: 0, failures: 0, suspended: 0 },
		);
		const executable = rollup.successes + rollup.failures;
		if (executable === 0) {
			return rollup.suspended > 0 ? "capture_suspended" : "unknown";
		}
		if (rollup.successes === executable) return "up";
		if (rollup.successes === 0) return "down";
		return "degraded";
	});
}

export function ServiceMonitorDetailsPage() {
	const { monitorId } = useParams({ from: "/app/monitors/$monitorId" });
	const adminToken = readAdminToken();
	const runtime = useAppRuntime();
	const capability = useApiCapability("admin.service-monitors");
	const queryClient = useQueryClient();
	const navigate = useNavigate();
	const { pushToast } = useToast();
	const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
	const monitorQuery = useQuery({
		queryKey: ["adminServiceMonitor", adminToken, monitorId],
		enabled: adminToken.length > 0 && capability.available,
		queryFn: ({ signal }) =>
			fetchAdminServiceMonitor(adminToken, monitorId, signal),
	});
	const statusQuery = useQuery({
		queryKey: ["adminServiceMonitorStatus", adminToken, monitorId],
		enabled: adminToken.length > 0 && capability.available,
		queryFn: ({ signal }) =>
			fetchAdminServiceMonitorStatus(adminToken, monitorId, signal),
		refetchInterval: runtime.isOnline ? 15_000 : false,
	});
	const historyQuery = useQuery({
		queryKey: ["adminServiceMonitorHistory", adminToken, monitorId, "24h"],
		enabled: adminToken.length > 0 && capability.available,
		queryFn: ({ signal }) =>
			fetchAdminServiceMonitorHistory(
				adminToken,
				monitorId,
				{ from: Math.floor(Date.now() / 1000) - 86_400, limit: 1_500 },
				signal,
			),
		refetchInterval: runtime.isOnline ? 30_000 : false,
	});

	const invalidate = () =>
		queryClient.invalidateQueries({
			queryKey: ["adminServiceMonitors", adminToken],
		});
	const runMutation = useMutation({
		mutationFn: () => runAdminServiceMonitor(adminToken, monitorId),
		onSuccess: (run) => {
			pushToast({ variant: "success", message: "Check started." });
			statusQuery.refetch();
			pushToast({ variant: "info", message: `Run ${run.run_id} is queued.` });
		},
		onError: (error) =>
			pushToast({ variant: "error", message: errorMessage(error) }),
	});
	const lifecycleMutation = useMutation({
		mutationFn: (lifecycle: "active" | "paused") => {
			if (!monitorQuery.data) throw new Error("Monitor is not loaded.");
			return patchAdminServiceMonitor(adminToken, monitorId, {
				expected_revision: monitorQuery.data.revision,
				lifecycle,
			});
		},
		onSuccess: (monitor) => {
			queryClient.setQueryData(
				["adminServiceMonitor", adminToken, monitorId],
				monitor,
			);
			invalidate();
			pushToast({
				variant: "success",
				message:
					monitor.lifecycle === "paused"
						? "Monitor paused."
						: "Monitor resumed.",
			});
		},
		onError: (error) =>
			pushToast({ variant: "error", message: errorMessage(error) }),
	});
	const deleteMutation = useMutation({
		mutationFn: () => {
			if (!monitorQuery.data) throw new Error("Monitor is not loaded.");
			return deleteAdminServiceMonitor(
				adminToken,
				monitorId,
				monitorQuery.data.revision,
			);
		},
		onSuccess: () => {
			invalidate();
			pushToast({
				variant: "success",
				message:
					"Monitor deleted. Historical observations remain until retention expires.",
			});
			navigate({ to: "/monitors" });
		},
		onError: (error) =>
			pushToast({ variant: "error", message: errorMessage(error) }),
	});

	if (!adminToken)
		return <PageState variant="empty" title="Admin token required" />;
	if (capability.unavailable)
		return (
			<CapabilityUnavailableState
				title="Service monitoring unavailable"
				reason={capability.reason}
			/>
		);
	if (monitorQuery.isLoading)
		return <PageState variant="loading" title="Loading service monitor" />;
	if (monitorQuery.isError || !monitorQuery.data)
		return (
			<PageState
				variant="error"
				title="Failed to load service monitor"
				error={monitorQuery.error}
			/>
		);

	const monitor = monitorQuery.data;
	const status = statusQuery.data;
	const history = historyQuery.data;
	const quality = history?.quality ?? status?.quality;
	const qualityNotice = quality ? qualityMessage(quality) : null;
	const historyRollup = history?.points.reduce(
		(total, point) => ({
			expected: total.expected + point.rollup.expected,
			executed: total.executed + point.rollup.executed,
			successes: total.successes + point.rollup.successes,
			failures: total.failures + point.rollup.failures,
			latencyCount: total.latencyCount + point.rollup.latency_count,
			latencySum: total.latencySum + point.rollup.latency_sum_ms,
		}),
		{
			expected: 0,
			executed: 0,
			successes: 0,
			failures: 0,
			latencyCount: 0,
			latencySum: 0,
		},
	);
	const availability =
		historyRollup && historyRollup.successes + historyRollup.failures > 0
			? (historyRollup.successes * 100) /
				(historyRollup.successes + historyRollup.failures)
			: null;
	const averageLatency =
		historyRollup && historyRollup.latencyCount > 0
			? Math.round(historyRollup.latencySum / historyRollup.latencyCount)
			: null;
	const timelineSlots = history
		? historyTimelineSlots(history.points)
		: Array.from({ length: 24 }, () => "unknown" as const);

	return (
		<div className="space-y-6">
			<PageHeader
				title={monitor.name}
				description={`${monitorKind(monitor.target).toUpperCase()} · ${monitorTargetLabel(
					monitor.target,
				)}`}
				meta={
					<>
						<ServiceMonitorStatusBadge
							status={status?.status ?? "unknown"}
							stale={status?.stale}
						/>
						<Badge variant="ghost" size="sm">
							rev {monitor.revision}
						</Badge>
					</>
				}
				actions={
					<div className="flex flex-wrap gap-2">
						<Button
							variant="secondary"
							loading={runMutation.isPending}
							disabled={runtime.isReadOnly || monitor.lifecycle === "deleted"}
							onClick={() => runMutation.mutate()}
						>
							Run now
						</Button>
						<Button
							asChild
							variant="secondary"
							disabled={runtime.isReadOnly || monitor.lifecycle === "deleted"}
						>
							<Link to="/monitors/$monitorId/edit" params={{ monitorId }}>
								Edit
							</Link>
						</Button>
						{monitor.lifecycle !== "deleted" ? (
							<Button
								variant="secondary"
								loading={lifecycleMutation.isPending}
								disabled={runtime.isReadOnly}
								onClick={() =>
									lifecycleMutation.mutate(
										monitor.lifecycle === "active" ? "paused" : "active",
									)
								}
							>
								{monitor.lifecycle === "active" ? "Pause" : "Resume"}
							</Button>
						) : null}
						<Button
							variant="danger"
							disabled={runtime.isReadOnly || monitor.lifecycle === "deleted"}
							onClick={() => setDeleteDialogOpen(true)}
						>
							Delete
						</Button>
					</div>
				}
			/>
			{status?.capture.suspended ? (
				<ReadStateBanner
					tone="warning"
					title="Capture suspended"
					description={[
						"The local delivery journal is above its safety limit.",
						"New scheduled checks stay stopped until it recovers.",
					].join(" ")}
				/>
			) : null}
			{qualityNotice ? (
				<ReadStateBanner
					tone="warning"
					title={`History ${quality?.replace("_", " ")}`}
					description={qualityNotice}
				/>
			) : null}
			{statusQuery.isError ? (
				<ReadStateBanner
					tone="warning"
					title="Status refresh failed"
					description={errorMessage(statusQuery.error)}
				/>
			) : null}
			<Card>
				<CardContent className="grid gap-4 py-5 sm:grid-cols-2 lg:grid-cols-4">
					<Metric
						label="Current status"
						value={status?.status.replace("_", " ") ?? "Unknown"}
						detail={
							status ? age(status.freshness_seconds) : "Waiting for status"
						}
					/>
					<Metric
						label="24h availability"
						value={percent(availability)}
						detail="Successful / executable checks"
					/>
					<Metric
						label="24h coverage"
						value={percent(history?.coverage_percent)}
						detail={
							history
								? `${historyRollup?.executed ?? 0}/${historyRollup?.expected ?? 0} scheduled slots`
								: "Waiting for history"
						}
					/>
					<Metric
						label="Average latency"
						value={averageLatency === null ? "-" : `${averageLatency} ms`}
						detail={
							history?.resolution
								? `${history.resolution} rollup`
								: "No latency samples"
						}
					/>
					<div className="border-t border-border/70 pt-4 sm:col-span-2 lg:col-span-4">
						<div className="flex items-center justify-between gap-3">
							<p className="text-xs font-medium text-muted-foreground">
								24 hour check history
							</p>
							<p className="text-xs text-muted-foreground">
								24 hours ago <span className="px-1">-</span> now
							</p>
						</div>
						<ServiceMonitorUptimeBar
							className="mt-2"
							label={`${monitor.name} 24 hour check history`}
							slots={timelineSlots}
						/>
					</div>
				</CardContent>
			</Card>
			<Card>
				<CardHeader>
					<CardTitle>Availability and latency</CardTitle>
				</CardHeader>
				<CardContent>
					{historyQuery.isLoading ? (
						<p className="py-10 text-center text-sm text-muted-foreground">
							Loading history...
						</p>
					) : historyQuery.isError ? (
						<p className="py-10 text-center text-sm text-destructive">
							{errorMessage(historyQuery.error)}
						</p>
					) : history ? (
						<ServiceMonitorHistoryChart history={history} />
					) : null}
				</CardContent>
			</Card>
			<Card>
				<CardHeader>
					<CardTitle>Observer results</CardTitle>
				</CardHeader>
				<CardContent className="p-0">
					<DataTable
						headers={[
							{ key: "node", label: "Observer" },
							{ key: "state", label: "State" },
							{ key: "latest", label: "Latest scheduled observation" },
							{ key: "capability", label: "ICMP" },
						]}
					>
						{(status?.observers ?? []).map((observer) => (
							<TableRow key={observer.node_id}>
								<TableCell className="font-medium">
									{observer.node_id}
								</TableCell>
								<TableCell>
									<ServiceMonitorStatusBadge status={observer.state} />
								</TableCell>
								<TableCell>
									<span className="text-sm">
										{observer.latest
											? `${observer.latest.outcome}${
													observer.latest.latency_ms === null ||
													observer.latest.latency_ms === undefined
														? ""
														: ` · ${observer.latest.latency_ms} ms`
												}`
											: "No local observation"}
									</span>
									<p className="mt-0.5 text-xs text-muted-foreground">
										{time(observer.latest?.observed_at_unix_seconds)}
									</p>
								</TableCell>
								<TableCell>
									<Badge
										variant={
											observer.icmp_supported === true ? "success" : "ghost"
										}
										size="sm"
									>
										{observer.icmp_supported === true
											? "Supported"
											: observer.icmp_supported === false
												? "Unsupported"
												: "Unknown"}
									</Badge>
								</TableCell>
							</TableRow>
						))}
					</DataTable>
				</CardContent>
			</Card>
			<ConfirmDialog
				open={deleteDialogOpen}
				title="Delete service monitor?"
				description="Scheduled checks stop after the next effective Slot. Existing history is retained."
				confirmLabel="Delete monitor"
				onCancel={() => setDeleteDialogOpen(false)}
				onConfirm={() => {
					setDeleteDialogOpen(false);
					deleteMutation.mutate();
				}}
				footer={
					<div className="flex justify-end gap-2">
						<Button variant="ghost" onClick={() => setDeleteDialogOpen(false)}>
							Cancel
						</Button>
						<Button
							variant="danger"
							loading={deleteMutation.isPending}
							onClick={() => {
								setDeleteDialogOpen(false);
								deleteMutation.mutate();
							}}
						>
							Delete monitor
						</Button>
					</div>
				}
			/>
		</div>
	);
}
