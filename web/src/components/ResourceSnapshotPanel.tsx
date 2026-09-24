import type { EChartsOption } from "echarts";
import ReactEChartsCore from "echarts-for-react/lib/core";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { SVGRenderer } from "echarts/renderers";
import { useEffect, useMemo, useState } from "react";

import type {
	NodeResourceHistoryMetric,
	ResourceRole,
	ResourceSnapshot,
	RuntimeResourceHistoryMetric,
} from "../api/adminResources";
import { classifyResourceError } from "../utils/resourceError";
import { Icon } from "./Icon";
import { CapabilityUnavailableState, PageState } from "./PageState";
import {
	type EChartsThemePalette,
	STATIC_LINE_SERIES_EMPHASIS,
	createThemedTooltipSurface,
	useEChartsThemePalette,
} from "./echarts-theme";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";

echarts.use([GridComponent, TooltipComponent, LineChart, SVGRenderer]);

const RESOURCE_HISTORY_CHART_HEIGHT = 152;
const RESOURCE_HISTORY_COMPARISON_COLOR = "#3478c6";

type ResourceHistoryValueUnit =
	| "bytes"
	| "bytes_per_second"
	| "count"
	| "percent";

type ResourceHistoryChartSeries = {
	metric: string;
	name: string;
};

type ResourceHistoryChart = {
	key: string;
	title: string;
	ariaLabel: string;
	emptyMessage: string;
	unit: ResourceHistoryValueUnit;
	series: readonly ResourceHistoryChartSeries[];
};

export const RESOURCE_HISTORY_CHARTS = [
	{
		key: "cpu_busy_percent",
		title: "CPU busy history",
		ariaLabel: "CPU busy history chart",
		emptyMessage: "No CPU rollup points yet.",
		unit: "percent",
		series: [{ metric: "cpu_busy_percent", name: "CPU busy" }],
	},
	{
		key: "memory_available_bytes",
		title: "Available memory history",
		ariaLabel: "Available memory history chart",
		emptyMessage: "No memory rollup points yet.",
		unit: "bytes",
		series: [{ metric: "memory_available_bytes", name: "Available memory" }],
	},
	{
		key: "filesystem.root.used_percent",
		title: "Root filesystem use history",
		ariaLabel: "Root filesystem use history chart",
		emptyMessage: "No root filesystem rollup points yet.",
		unit: "percent",
		series: [
			{ metric: "filesystem.root.used_percent", name: "Root filesystem use" },
		],
	},
	{
		key: "cpu_iowait_percent",
		title: "I/O wait history",
		ariaLabel: "I/O wait history chart",
		emptyMessage: "No I/O wait rollup points yet.",
		unit: "percent",
		series: [{ metric: "cpu_iowait_percent", name: "I/O wait" }],
	},
] as const satisfies readonly ResourceHistoryChart[];

export const RUNTIME_RESOURCE_HISTORY_CHARTS = [
	{
		key: "cpu_percent",
		title: "CPU",
		ariaLabel: "Runtime CPU history chart",
		emptyMessage: "No CPU rollup points yet.",
		unit: "percent",
		series: [{ metric: "cpu_percent", name: "CPU" }],
	},
	{
		key: "memory",
		title: "Memory (RSS / PSS)",
		ariaLabel: "Runtime memory history chart",
		emptyMessage: "No memory rollup points yet.",
		unit: "bytes",
		series: [
			{ metric: "rss_bytes", name: "RSS" },
			{ metric: "pss_bytes", name: "PSS" },
		],
	},
	{
		key: "throughput",
		title: "Throughput (Read / Write)",
		ariaLabel: "Runtime throughput history chart",
		emptyMessage: "No throughput rollup points yet.",
		unit: "bytes_per_second",
		series: [
			{ metric: "read_bytes_per_second", name: "Read" },
			{ metric: "write_bytes_per_second", name: "Write" },
		],
	},
	{
		key: "fd_count",
		title: "File descriptors",
		ariaLabel: "Runtime file descriptor history chart",
		emptyMessage: "No file descriptor rollup points yet.",
		unit: "count",
		series: [{ metric: "fd_count", name: "File descriptors" }],
	},
	{
		key: "thread_count",
		title: "Threads",
		ariaLabel: "Runtime thread history chart",
		emptyMessage: "No thread rollup points yet.",
		unit: "count",
		series: [{ metric: "thread_count", name: "Threads" }],
	},
] as const satisfies readonly ResourceHistoryChart[];

function useRetryCooldown(error: unknown, isOnline: boolean) {
	const diagnostic = classifyResourceError(error, { isOnline });
	const [now, setNow] = useState(() => Date.now());
	const retryAt = diagnostic.retryDeadlineAt ?? null;

	useEffect(() => {
		if (!retryAt) return;
		const timer = window.setInterval(() => setNow(Date.now()), 1000);
		return () => window.clearInterval(timer);
	}, [retryAt]);

	const remainingSeconds = retryAt
		? Math.max(0, Math.ceil((retryAt - now) / 1000))
		: 0;
	return { diagnostic, remainingSeconds };
}

function formatResourceTimestamp(timestamp: number): string {
	if (!timestamp) return "unavailable";
	const date = new Date(timestamp);
	return Number.isNaN(date.getTime()) ? "unavailable" : date.toLocaleString();
}

function formatResourceAge(timestamp: number, now = Date.now()): string {
	if (!timestamp) return "unknown";
	const seconds = Math.max(0, Math.floor((now - timestamp) / 1000));
	if (seconds < 60) return "less than a minute";
	if (seconds < 3600) return `${Math.floor(seconds / 60)} min`;
	return `${Math.floor(seconds / 3600)} hr`;
}

function ResourceDiagnosticBanner(props: {
	error: unknown;
	isOnline: boolean;
	isFetching: boolean;
	snapshot?: ResourceSnapshot;
	dataUpdatedAt?: number;
	onRetry: () => void;
	onOpenMeshStatus?: () => void;
}) {
	const { diagnostic, remainingSeconds } = useRetryCooldown(
		props.error,
		props.isOnline,
	);
	const canRetry =
		props.isOnline && diagnostic.retryable && remainingSeconds === 0;
	const [now, setNow] = useState(() => Date.now());

	useEffect(() => {
		if (!props.snapshot || !props.dataUpdatedAt) return;
		const timer = window.setInterval(() => setNow(Date.now()), 1_000);
		return () => window.clearInterval(timer);
	}, [props.dataUpdatedAt, props.snapshot]);
	const isPeerLayer = [
		"peer_transport",
		"peer_protocol",
		"circuit_breaker",
	].includes(diagnostic.layer);

	return (
		<div
			className="rounded-lg border border-warning/40 bg-warning/10 p-4"
			role="alert"
		>
			<div className="flex flex-wrap items-start justify-between gap-3">
				<div className="min-w-0 space-y-1">
					<div className="flex flex-wrap items-center gap-2">
						<p className="font-semibold">{diagnostic.title}</p>
						<Badge variant="warning">{diagnostic.layer}</Badge>
					</div>
					<p className="max-w-3xl text-sm text-muted-foreground">
						{diagnostic.description}
					</p>
					{props.snapshot ? (
						<p className="text-xs text-muted-foreground">
							Showing the last successful snapshot. Observed{" "}
							{props.snapshot.observed_at}, last successful read{" "}
							{formatResourceTimestamp(props.dataUpdatedAt ?? 0)}, age{" "}
							{formatResourceAge(props.dataUpdatedAt ?? 0, now)}.
						</p>
					) : null}
				</div>
				<div className="flex shrink-0 flex-wrap gap-2">
					<Button
						aria-label="Retry resource read"
						disabled={!canRetry || props.isFetching}
						onClick={props.onRetry}
						size="sm"
						variant="outline"
					>
						<Icon name="tabler:refresh" size={15} />
						{remainingSeconds > 0
							? `Retry in ${remainingSeconds}s`
							: props.isFetching
								? "Retrying"
								: "Retry"}
					</Button>
					{isPeerLayer && props.onOpenMeshStatus ? (
						<Button
							aria-label="Open Mesh status"
							onClick={props.onOpenMeshStatus}
							size="sm"
							variant="ghost"
						>
							<Icon name="tabler:activity" size={15} />
							Mesh status
						</Button>
					) : null}
				</div>
			</div>
			<div className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
				<span>Dispatch: {diagnostic.dispatchState}</span>
				<span>Confidence: {diagnostic.confidence}</span>
				{diagnostic.supportId ? (
					<span>Support: {diagnostic.supportId}</span>
				) : null}
			</div>
		</div>
	);
}

type ResourceHistoryPoint = {
	observed_at: string;
	value?: number | null;
};

function cpuSampleTimestamp(observedAt: string): number | null {
	const timestamp = Date.parse(observedAt);
	return Number.isNaN(timestamp) ? null : timestamp;
}

function formatCpuAxisTime(value: number): string {
	const timestamp = new Date(value);
	if (Number.isNaN(timestamp.getTime())) return "";
	return timestamp.toLocaleTimeString([], {
		hour: "2-digit",
		minute: "2-digit",
	});
}

function resourceBytesText(value?: number) {
	if (value === undefined) return "unsupported";
	if (value < 1024) return `${value} B`;
	const units = ["KiB", "MiB", "GiB", "TiB"];
	let scaled = value;
	let unit = "B";
	for (const next of units) {
		if (scaled < 1024) break;
		scaled /= 1024;
		unit = next;
	}
	return `${scaled.toFixed(1)} ${unit}`;
}

function resourceHistoryAxisFormatter(unit: ResourceHistoryValueUnit) {
	if (unit === "percent") return (value: number) => `${value}%`;
	if (unit === "count") return (value: number) => value.toFixed(0);
	if (unit === "bytes_per_second") {
		return (value: number) => `${resourceBytesText(value)}/s`;
	}
	return (value: number) => resourceBytesText(value);
}

export function buildResourceHistoryChartOption(
	historyByMetric: Partial<Record<string, ResourceHistoryPoint[]>>,
	palette: EChartsThemePalette,
	chart: ResourceHistoryChart,
): EChartsOption {
	const timestamps = Array.from(
		new Set(
			chart.series
				.flatMap((series) => historyByMetric[series.metric] ?? [])
				.flatMap((point) => {
					const timestamp = cpuSampleTimestamp(point.observed_at);
					return timestamp === null ? [] : [timestamp];
				}),
		),
	).sort((left, right) => left - right);
	return {
		animationDuration: 220,
		grid: { top: 24, right: 18, bottom: 36, left: 42 },
		tooltip: {
			...createThemedTooltipSurface(palette),
			trigger: "axis",
			confine: true,
		},
		xAxis: {
			type: "time",
			min: timestamps.at(0),
			max: timestamps.at(-1),
			axisLabel: {
				color: palette.axis,
				formatter: formatCpuAxisTime,
				hideOverlap: true,
				margin: 8,
			},
			axisLine: { lineStyle: { color: palette.grid } },
			axisTick: { show: false },
			splitLine: { show: false },
			splitNumber: 6,
		},
		yAxis: {
			type: "value",
			min: 0,
			...(chart.unit === "percent" ? { max: 100 } : {}),
			axisLabel: {
				color: palette.axis,
				formatter: resourceHistoryAxisFormatter(chart.unit),
			},
			axisLine: { show: false },
			axisTick: { show: false },
			splitLine: { lineStyle: { color: palette.grid } },
			splitNumber: 4,
		},
		series: chart.series.map((series, index) => {
			const primary = index === 0;
			const samples = (historyByMetric[series.metric] ?? []).flatMap(
				(point) => {
					const timestamp = cpuSampleTimestamp(point.observed_at);
					return timestamp === null ? [] : [[timestamp, point.value ?? null]];
				},
			);
			return {
				name: series.name,
				type: "line",
				data: samples,
				step: "end",
				smooth: false,
				connectNulls: false,
				showSymbol: false,
				emphasis: STATIC_LINE_SERIES_EMPHASIS,
				lineStyle: primary
					? { width: 2, color: palette.primary, join: "round" }
					: {
							width: 2,
							color: RESOURCE_HISTORY_COMPARISON_COLOR,
							join: "round",
						},
				...(primary
					? { areaStyle: { color: palette.primary, opacity: 0.18 } }
					: {}),
			};
		}),
	};
}

function ResourceHistoryChart(props: {
	chart: ResourceHistoryChart;
	historyByMetric: Partial<Record<string, ResourceHistoryPoint[]>>;
	error?: unknown;
	isOnline: boolean;
	onRetry?: () => void;
}) {
	const palette = useEChartsThemePalette();
	const { diagnostic, remainingSeconds } = useRetryCooldown(
		props.error,
		props.isOnline,
	);
	const option = useMemo(
		() =>
			buildResourceHistoryChartOption(
				props.historyByMetric,
				palette,
				props.chart,
			),
		[palette, props.chart, props.historyByMetric],
	);
	const hasSamples = props.chart.series.some((series) =>
		(props.historyByMetric[series.metric] ?? []).some(
			(point) => point.value !== null && point.value !== undefined,
		),
	);

	if (!hasSamples) {
		if (props.error) {
			return (
				<div className="space-y-2 py-7" role="alert">
					<p className="text-sm font-medium">History unavailable</p>
					<p className="text-xs text-muted-foreground">
						{diagnostic.description}
					</p>
					{props.onRetry && diagnostic.layer !== "unsupported" ? (
						<Button
							disabled={
								!props.isOnline || !diagnostic.retryable || remainingSeconds > 0
							}
							onClick={props.onRetry}
							size="sm"
							variant="outline"
						>
							<Icon name="tabler:refresh" size={15} />
							{remainingSeconds > 0
								? `Retry in ${remainingSeconds}s`
								: "Retry history"}
						</Button>
					) : null}
				</div>
			);
		}
		return (
			<p className="py-10 text-xs text-muted-foreground">
				{props.chart.emptyMessage}
			</p>
		);
	}

	return (
		<>
			<div aria-label={props.chart.ariaLabel} className="w-full">
				<ReactEChartsCore
					echarts={echarts}
					option={option}
					notMerge
					lazyUpdate
					autoResize
					style={{ height: RESOURCE_HISTORY_CHART_HEIGHT, width: "100%" }}
					opts={{ renderer: "svg" }}
				/>
			</div>
			{props.error ? (
				<p className="flex flex-wrap items-center gap-2 text-xs text-warning">
					<span>Refresh failed; showing the last successful points.</span>
					{props.onRetry && diagnostic.layer !== "unsupported" ? (
						<button
							className={[
								"font-medium underline underline-offset-2",
								"disabled:cursor-not-allowed disabled:no-underline disabled:opacity-60",
							].join(" ")}
							disabled={
								!props.isOnline || !diagnostic.retryable || remainingSeconds > 0
							}
							onClick={props.onRetry}
							type="button"
						>
							{remainingSeconds > 0
								? `Retry in ${remainingSeconds}s`
								: "Retry history"}
						</button>
					) : null}
				</p>
			) : null}
		</>
	);
}

function ResourceHistoryCard(props: {
	chart: ResourceHistoryChart;
	historyByMetric: Partial<Record<string, ResourceHistoryPoint[]>>;
	error?: unknown;
	isOnline: boolean;
	onRetry?: () => void;
}) {
	const palette = useEChartsThemePalette();
	return (
		<div className="rounded-lg border border-border/60 p-3">
			<div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
				<p className="font-semibold">{props.chart.title}</p>
				{props.chart.series.length > 1 ? (
					<div
						aria-label={`${props.chart.title} legend`}
						className="flex items-center gap-3 text-xs text-muted-foreground"
					>
						{props.chart.series.map((series, index) => (
							<span
								className="inline-flex items-center gap-1.5"
								key={series.metric}
							>
								<span
									aria-hidden="true"
									className="h-0.5 w-3 shrink-0"
									style={{
										backgroundColor:
											index === 0
												? palette.primary
												: RESOURCE_HISTORY_COMPARISON_COLOR,
									}}
								/>
								{series.name}
							</span>
						))}
					</div>
				) : null}
			</div>
			<ResourceHistoryChart
				chart={props.chart}
				error={props.error}
				historyByMetric={props.historyByMetric}
				isOnline={props.isOnline}
				onRetry={props.onRetry}
			/>
		</div>
	);
}

function resourceMeasurementText(measurement: {
	capability: string;
	value?: number;
	reason_code?: string;
}) {
	if (measurement.value === undefined) {
		return measurement.capability === "unsupported"
			? `unsupported${measurement.reason_code ? ` (${measurement.reason_code})` : ""}`
			: measurement.capability;
	}
	return `${measurement.value.toFixed(1)}${
		measurement.capability === "partial" ? " (partial)" : ""
	}`;
}

function ResourceMetric(props: {
	label: string;
	measurement: { capability: string; value?: number; reason_code?: string };
	bytes?: boolean;
}) {
	return (
		<div className="min-w-0 rounded-lg border border-border/60 bg-muted/20 p-3">
			<p className="text-xs text-muted-foreground">{props.label}</p>
			<p className="break-words font-mono text-sm leading-tight">
				{props.bytes
					? resourceBytesText(props.measurement.value)
					: resourceMeasurementText(props.measurement)}
			</p>
			<p className="text-xs text-muted-foreground">
				{props.measurement.capability}
			</p>
		</div>
	);
}

export function ResourceSnapshotPanel(props: {
	snapshot: ResourceSnapshot;
	historyByMetric: Partial<
		Record<
			NodeResourceHistoryMetric,
			Array<{ observed_at: string; value?: number | null }>
		>
	>;
	runtimeHistoryByMetric: Partial<
		Record<
			RuntimeResourceHistoryMetric,
			Array<{ observed_at: string; value?: number | null }>
		>
	>;
	historyErrorByMetric?: Partial<Record<NodeResourceHistoryMetric, unknown>>;
	runtimeHistoryErrorByMetric?: Partial<
		Record<RuntimeResourceHistoryMetric, unknown>
	>;
	isOnline?: boolean;
	onRetryHistory?: (metric: NodeResourceHistoryMetric) => void;
	onRetryRuntimeHistory?: (metric: RuntimeResourceHistoryMetric) => void;
	selectedRuntimeRole: ResourceRole | null;
	onRuntimeDetailsChange: (role: ResourceRole | null) => void;
}) {
	const { snapshot } = props;
	const isOnline = props.isOnline ?? true;
	return (
		<div className="space-y-4">
			<div className="flex flex-wrap items-center justify-between gap-2">
				<div>
					<p className="text-sm text-muted-foreground">
						Domain:{" "}
						<span className="font-mono">{snapshot.resource_domain}</span> ·
						capture {snapshot.capture_state}
					</p>
					<p className="text-xs text-muted-foreground">
						Observed {snapshot.observed_at}
					</p>
				</div>
				<Badge
					variant={
						snapshot.capability === "supported"
							? "success"
							: snapshot.capability === "partial"
								? "warning"
								: "outline"
					}
				>
					{snapshot.capability}
				</Badge>
			</div>
			<div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
				<ResourceMetric
					label="CPU busy %"
					measurement={snapshot.domain.cpu_busy_percent}
				/>
				<ResourceMetric
					label="I/O wait %"
					measurement={snapshot.domain.cpu_iowait_percent}
				/>
				<ResourceMetric label="Load 1m" measurement={snapshot.domain.load1} />
				<ResourceMetric
					label="Available memory"
					measurement={snapshot.domain.memory_available_bytes}
					bytes
				/>
			</div>
			<div className="rounded-lg border border-border/60 p-3">
				<p className="mb-2 font-semibold">Filesystems</p>
				<div className="space-y-2 text-sm">
					{snapshot.domain.filesystems.map((filesystem) => (
						<div
							className="flex flex-wrap justify-between gap-2"
							key={filesystem.mount}
						>
							<span className="font-mono">{filesystem.mount}</span>
							<span>
								{filesystem.used_percent === undefined
									? "unsupported"
									: `${filesystem.used_percent.toFixed(1)}% used · ${resourceBytesText(
											filesystem.available_bytes,
										)} free`}
							</span>
							<span className="text-xs text-muted-foreground">
								inodes{" "}
								{filesystem.used_inode_percent === undefined
									? "unsupported"
									: `${filesystem.used_inode_percent.toFixed(1)}% used`}
							</span>
						</div>
					))}
				</div>
			</div>
			<div className="grid gap-3 lg:grid-cols-2">
				{RESOURCE_HISTORY_CHARTS.map((chart) => (
					<ResourceHistoryCard
						chart={chart}
						error={chart.series
							.map((series) => props.historyErrorByMetric?.[series.metric])
							.find(Boolean)}
						historyByMetric={props.historyByMetric}
						isOnline={isOnline}
						onRetry={() => {
							for (const series of chart.series) {
								props.onRetryHistory?.(
									series.metric as NodeResourceHistoryMetric,
								);
							}
						}}
						key={chart.key}
					/>
				))}
			</div>
			<div>
				<p className="mb-2 font-semibold">Managed runtime roles</p>
				<div className="grid gap-3 sm:grid-cols-2">
					{snapshot.runtimes.map((runtime) => (
						<button
							aria-label={`Open ${runtime.role} resource details`}
							aria-expanded={props.selectedRuntimeRole === runtime.role}
							className={[
								"rounded-lg border border-border/60 bg-muted/20 p-3 text-left",
								"transition-colors hover:bg-muted/40 focus-visible:outline-none",
								"focus-visible:ring-2 focus-visible:ring-ring",
							].join(" ")}
							key={runtime.role}
							onClick={() =>
								props.onRuntimeDetailsChange(
									props.selectedRuntimeRole === runtime.role
										? null
										: runtime.role,
								)
							}
							type="button"
						>
							<div className="mb-2 flex items-center justify-between gap-2">
								<div className="flex items-center gap-2">
									<span className="font-semibold">{runtime.role}</span>
									<Badge
										variant={
											runtime.state === "managed" ? "secondary" : "outline"
										}
									>
										{runtime.state}
									</Badge>
								</div>
								<Icon
									className={
										props.selectedRuntimeRole === runtime.role
											? "rotate-90 transition-transform"
											: "transition-transform"
									}
									name="tabler:chevron-right"
									size={16}
								/>
							</div>
							<div className="grid grid-cols-2 gap-2 text-xs">
								<ResourceMetric
									label="CPU"
									measurement={runtime.metrics.cpu_percent}
								/>
								<ResourceMetric
									label="RSS"
									measurement={runtime.metrics.rss_bytes}
									bytes
								/>
							</div>
						</button>
					))}
				</div>
			</div>
			{props.selectedRuntimeRole ? (
				<RuntimeResourceDetails
					historyByMetric={props.runtimeHistoryByMetric}
					historyErrorByMetric={props.runtimeHistoryErrorByMetric}
					isOnline={isOnline}
					onRetryHistory={props.onRetryRuntimeHistory}
					role={props.selectedRuntimeRole}
				/>
			) : null}
		</div>
	);
}

function RuntimeResourceDetails(props: {
	role: ResourceRole;
	historyByMetric: Partial<
		Record<
			RuntimeResourceHistoryMetric,
			Array<{ observed_at: string; value?: number | null }>
		>
	>;
	historyErrorByMetric?: Partial<Record<RuntimeResourceHistoryMetric, unknown>>;
	isOnline: boolean;
	onRetryHistory?: (metric: RuntimeResourceHistoryMetric) => void;
}) {
	return (
		<section aria-labelledby="runtime-resource-history-heading">
			<div className="mb-2">
				<p className="font-semibold" id="runtime-resource-history-heading">
					{props.role} resource history
				</p>
				<p className="text-xs text-muted-foreground">
					Fixed managed-runtime measurements only. Unavailable measurements
					remain explicit instead of being shown as zero.
				</p>
			</div>
			<div className="grid gap-3 lg:grid-cols-2">
				{RUNTIME_RESOURCE_HISTORY_CHARTS.map((chart) => (
					<ResourceHistoryCard
						chart={chart}
						error={chart.series
							.map(
								(series) =>
									props.historyErrorByMetric?.[
										series.metric as RuntimeResourceHistoryMetric
									],
							)
							.find(Boolean)}
						historyByMetric={props.historyByMetric}
						isOnline={props.isOnline}
						onRetry={() => {
							for (const series of chart.series) {
								props.onRetryHistory?.(
									series.metric as RuntimeResourceHistoryMetric,
								);
							}
						}}
						key={chart.key}
					/>
				))}
			</div>
		</section>
	);
}

export function ResourceTabContent(props: {
	capabilityUnavailable: boolean;
	capabilityReason?: string | null;
	isLoading: boolean;
	isError: boolean;
	error: unknown;
	isFetching: boolean;
	isOnline: boolean;
	onRetry: () => void;
	onOpenMeshStatus?: () => void;
	dataUpdatedAt?: number;
	snapshot?: ResourceSnapshot;
	historyByMetric: Partial<
		Record<
			NodeResourceHistoryMetric,
			Array<{ observed_at: string; value?: number | null }>
		>
	>;
	historyErrorByMetric?: Partial<Record<NodeResourceHistoryMetric, unknown>>;
	runtimeHistoryErrorByMetric?: Partial<
		Record<RuntimeResourceHistoryMetric, unknown>
	>;
	onRetryHistory?: (metric: NodeResourceHistoryMetric) => void;
	onRetryRuntimeHistory?: (metric: RuntimeResourceHistoryMetric) => void;
	runtimeHistoryByMetric: Partial<
		Record<
			RuntimeResourceHistoryMetric,
			Array<{ observed_at: string; value?: number | null }>
		>
	>;
	selectedRuntimeRole: ResourceRole | null;
	onRuntimeDetailsChange: (role: ResourceRole | null) => void;
}) {
	if (props.capabilityUnavailable) {
		return (
			<CapabilityUnavailableState
				title="Resource monitoring unavailable"
				reason={props.capabilityReason}
			/>
		);
	}
	if (props.isLoading && !props.snapshot) {
		return (
			<PageState
				variant="loading"
				title="Loading resources"
				description="Fetching the latest node resource snapshot."
			/>
		);
	}
	if (props.isError && !props.snapshot) {
		return (
			<div className="space-y-4">
				<ResourceDiagnosticBanner
					error={props.error}
					isFetching={props.isFetching}
					isOnline={props.isOnline}
					onOpenMeshStatus={props.onOpenMeshStatus}
					onRetry={props.onRetry}
				/>
				<PageState
					variant="empty"
					title="No resource snapshot available"
					description="There is no successful snapshot to retain for this page session."
				/>
			</div>
		);
	}
	if (props.snapshot) {
		return (
			<div className="space-y-4">
				{props.isError ? (
					<ResourceDiagnosticBanner
						dataUpdatedAt={props.dataUpdatedAt}
						error={props.error}
						isFetching={props.isFetching}
						isOnline={props.isOnline}
						onOpenMeshStatus={props.onOpenMeshStatus}
						onRetry={props.onRetry}
						snapshot={props.snapshot}
					/>
				) : null}
				<ResourceSnapshotPanel
					historyByMetric={props.historyByMetric}
					historyErrorByMetric={props.historyErrorByMetric}
					isOnline={props.isOnline}
					onRetryHistory={props.onRetryHistory}
					onRetryRuntimeHistory={props.onRetryRuntimeHistory}
					onRuntimeDetailsChange={props.onRuntimeDetailsChange}
					runtimeHistoryByMetric={props.runtimeHistoryByMetric}
					runtimeHistoryErrorByMetric={props.runtimeHistoryErrorByMetric}
					selectedRuntimeRole={props.selectedRuntimeRole}
					snapshot={props.snapshot}
				/>
			</div>
		);
	}
	return (
		<PageState
			variant="empty"
			title="No resource snapshot"
			description="The sampler has not produced a snapshot yet."
		/>
	);
}
