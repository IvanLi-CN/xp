import type { EChartsOption } from "echarts";
import ReactEChartsCore from "echarts-for-react/lib/core";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { SVGRenderer } from "echarts/renderers";
import { useMemo } from "react";

import type { ResourceSnapshot } from "../api/adminResources";
import { formatBackendError as formatErrorMessage } from "../utils/backendErrorMessage";
import { CapabilityUnavailableState, PageState } from "./PageState";
import { QueryErrorState } from "./QueryErrorState";
import {
	type EChartsThemePalette,
	STATIC_LINE_SERIES_EMPHASIS,
	createThemedTooltipSurface,
	useEChartsThemePalette,
} from "./echarts-theme";
import { Badge } from "./ui/badge";

echarts.use([GridComponent, TooltipComponent, LineChart, SVGRenderer]);

const CPU_HISTORY_CHART_HEIGHT = 152;

type ResourceHistoryPoint = {
	observed_at: string;
	value?: number | null;
};

function formatCpuHistoryAxisLabel(
	observedAt: string,
	index: number,
	pointCount: number,
): string {
	const timestamp = Date.parse(observedAt);
	if (Number.isNaN(timestamp)) return observedAt;
	const value = new Date(timestamp).toISOString();
	const firstTimestamp = pointCount > 0 ? index === 0 : false;
	const lastTimestamp = pointCount > 1 && index === pointCount - 1;
	const step = Math.max(1, Math.ceil((pointCount - 1) / 3));
	if (!firstTimestamp && !lastTimestamp && index % step !== 0) return "";
	return pointCount > 96
		? value.slice(5, 16).replace("T", " ")
		: value.slice(11, 16);
}

export function buildResourceCpuHistoryChartOption(
	historyPoints: ResourceHistoryPoint[],
	palette: EChartsThemePalette,
): EChartsOption {
	return {
		animation: false,
		grid: { top: 16, right: 12, bottom: 28, left: 40 },
		tooltip: {
			...createThemedTooltipSurface(palette),
			trigger: "axis",
			confine: true,
		},
		xAxis: {
			type: "category",
			boundaryGap: false,
			data: historyPoints.map((point) => point.observed_at),
			axisLabel: {
				color: palette.axis,
				hideOverlap: true,
				interval: 0,
				formatter: (value: string, index: number) =>
					formatCpuHistoryAxisLabel(value, index, historyPoints.length),
			},
			axisLine: { lineStyle: { color: palette.grid } },
			axisTick: { show: false },
		},
		yAxis: {
			type: "value",
			min: 0,
			max: 100,
			axisLabel: { color: palette.axis, formatter: "{value}%" },
			splitLine: { lineStyle: { color: palette.grid } },
		},
		series: [
			{
				name: "CPU busy",
				type: "line",
				data: historyPoints.map((point) => point.value ?? null),
				smooth: false,
				connectNulls: false,
				symbol: "none",
				emphasis: STATIC_LINE_SERIES_EMPHASIS,
				lineStyle: { color: palette.primary, width: 2, join: "round" },
			},
		],
	};
}

function ResourceCpuHistoryChart(props: {
	historyPoints: ResourceHistoryPoint[];
}) {
	const palette = useEChartsThemePalette();
	const option = useMemo(
		() => buildResourceCpuHistoryChartOption(props.historyPoints, palette),
		[palette, props.historyPoints],
	);
	const hasSamples = props.historyPoints.some(
		(point) => point.value !== null && point.value !== undefined,
	);

	if (!hasSamples) {
		return (
			<p className="py-10 text-xs text-muted-foreground">
				No CPU rollup points yet.
			</p>
		);
	}

	return (
		<div aria-label="CPU busy history chart" className="w-full">
			<ReactEChartsCore
				echarts={echarts}
				option={option}
				notMerge
				lazyUpdate
				style={{ height: CPU_HISTORY_CHART_HEIGHT, width: "100%" }}
				opts={{ renderer: "svg" }}
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
	historyPoints: Array<{ observed_at: string; value?: number | null }>;
}) {
	const { snapshot } = props;
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
			<div className="grid gap-3 lg:grid-cols-2">
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
				<div className="rounded-lg border border-border/60 p-3">
					<p className="font-semibold">CPU busy history</p>
					<ResourceCpuHistoryChart historyPoints={props.historyPoints} />
				</div>
			</div>
			<div>
				<p className="mb-2 font-semibold">Managed runtime roles</p>
				<div className="grid gap-3 sm:grid-cols-2">
					{snapshot.runtimes.map((runtime) => (
						<div
							className="rounded-lg border border-border/60 bg-muted/20 p-3"
							key={runtime.role}
						>
							<div className="mb-2 flex items-center justify-between gap-2">
								<span className="font-semibold">{runtime.role}</span>
								<Badge
									variant={
										runtime.state === "managed" ? "secondary" : "outline"
									}
								>
									{runtime.state}
								</Badge>
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
						</div>
					))}
				</div>
			</div>
		</div>
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
	snapshot?: ResourceSnapshot;
	historyPoints: Array<{ observed_at: string; value?: number | null }>;
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
			<QueryErrorState
				title="Failed to load resources"
				description={formatErrorMessage(props.error)}
				error={props.error}
				loading={props.isFetching}
				disabled={!props.isOnline}
				onRetry={props.onRetry}
			/>
		);
	}
	if (props.snapshot) {
		return (
			<ResourceSnapshotPanel
				snapshot={props.snapshot}
				historyPoints={props.historyPoints}
			/>
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
