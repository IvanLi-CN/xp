import type { EChartsOption } from "echarts";
import ReactEChartsCore from "echarts-for-react/lib/core";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { SVGRenderer } from "echarts/renderers";
import { useEffect, useMemo, useRef, useState } from "react";

import { cn } from "@/lib/utils";

import type {
	AdminNodeTcpConnectionsResponse,
	AdminTcpConnectionEndpointOption,
	AdminTcpConnectionUsageWindow,
} from "../api/adminTcpConnections";
import { Button } from "./Button";
import { PageState } from "./PageState";
import { alertClass } from "./ui-helpers";
import { Badge } from "./ui/badge";
import { Checkbox } from "./ui/checkbox";

echarts.use([GridComponent, TooltipComponent, LineChart, SVGRenderer]);

const SVG_RENDERER = { renderer: "svg" } as const;
const CHART_HEIGHT = 280;
const CHART_COLOR_FALLBACK = "var(--primary)";
const CHART_GRID_COLOR = "rgba(148, 163, 184, 0.10)";
const CHART_AXIS_COLOR = "rgba(148, 163, 184, 0.5)";
const TOOLTIP_BACKGROUND_FALLBACK = "rgb(34, 40, 52)";
const TOOLTIP_BORDER_FALLBACK = "rgba(148, 163, 184, 0.24)";
const TOOLTIP_FOREGROUND_FALLBACK = "rgb(226, 232, 240)";
const TOOLTIP_MUTED_FALLBACK = "rgb(148, 163, 184)";
const TOOLTIP_SHADOW_FALLBACK = "rgba(15, 23, 42, 0.28)";

type TcpConnectionUsageViewProps = {
	window: AdminTcpConnectionUsageWindow;
	onWindowChange: (window: AdminTcpConnectionUsageWindow) => void;
	report: Pick<
		AdminNodeTcpConnectionsResponse,
		| "window_start"
		| "window_end"
		| "warnings"
		| "endpoints"
		| "per_endpoint_series"
	>;
	isFetching?: boolean;
	tooltipPreviewMinute?: string | null;
};

type MinuteConnectionBreakdown = {
	minute: string;
	minuteMs: number;
	totalCount: number;
	endpointCounts: Record<string, number>;
};

type TooltipAxisParam = {
	axisValue?: number | string;
	marker?: string;
};

type TooltipThemePalette = {
	background: string;
	border: string;
	foreground: string;
	muted: string;
	shadow: string;
};

type EChartsReactRef = InstanceType<typeof ReactEChartsCore>;
type TooltipPreviewData = {
	minuteMs: number;
	totalCount: number;
	endpointRows: Array<{
		endpointId: string;
		label: string;
		count: number;
	}>;
};

function formatDateTime(value: string): string {
	const dt = new Date(value);
	if (Number.isNaN(dt.getTime())) return value;
	return dt.toLocaleString();
}

function formatTooltipDateTime(valueMs: number): string {
	const dt = new Date(valueMs);
	if (Number.isNaN(dt.getTime())) return "";
	return new Intl.DateTimeFormat("sv-SE", {
		year: "numeric",
		month: "2-digit",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
		hour12: false,
	})
		.format(dt)
		.replace(",", "");
}

function formatConnectionCount(count: number): string {
	return `${count} connection${count === 1 ? "" : "s"}`;
}

function escapeHtml(value: string): string {
	return value
		.replaceAll("&", "&amp;")
		.replaceAll("<", "&lt;")
		.replaceAll(">", "&gt;")
		.replaceAll('"', "&quot;")
		.replaceAll("'", "&#39;");
}

function coerceTooltipMinuteMs(
	axisValue: number | string | undefined,
): number | null {
	if (typeof axisValue === "number" && Number.isFinite(axisValue)) {
		return axisValue;
	}
	if (typeof axisValue === "string") {
		const parsed = Number(axisValue);
		if (Number.isFinite(parsed)) return parsed;
		const dateMs = new Date(axisValue).getTime();
		if (!Number.isNaN(dateMs)) return dateMs;
	}
	return null;
}

function formatAxisTime(
	value: number,
	window: AdminTcpConnectionUsageWindow,
): string {
	const dt = new Date(value);
	if (Number.isNaN(dt.getTime())) return "";
	if (window === "24h") {
		return dt.toLocaleTimeString([], {
			hour: "2-digit",
			minute: "2-digit",
		});
	}
	return dt.toLocaleString([], {
		month: "short",
		day: "numeric",
	});
}

function shouldCompactTimeAxis(): boolean {
	if (typeof window === "undefined") return false;
	return window.innerWidth < 480;
}

function endpointLabel(endpoint: AdminTcpConnectionEndpointOption): string {
	return `${endpoint.endpoint_tag} :${endpoint.port}`;
}

function computeDefaultSelection(
	endpoints: AdminTcpConnectionEndpointOption[],
): string[] {
	return endpoints.map((endpoint) => endpoint.endpoint_id);
}

export function buildMinuteConnectionBreakdowns(
	selectedSeries: AdminNodeTcpConnectionsResponse["per_endpoint_series"],
): MinuteConnectionBreakdown[] {
	const byMinute = new Map<number, MinuteConnectionBreakdown>();
	for (const endpointSeries of selectedSeries) {
		for (const point of endpointSeries.series) {
			const minuteMs = new Date(point.minute).getTime();
			if (Number.isNaN(minuteMs)) continue;
			const current = byMinute.get(minuteMs);
			if (current) {
				current.totalCount += point.count;
				current.endpointCounts[endpointSeries.endpoint_id] =
					(current.endpointCounts[endpointSeries.endpoint_id] ?? 0) +
					point.count;
				continue;
			}
			byMinute.set(minuteMs, {
				minute: point.minute,
				minuteMs,
				totalCount: point.count,
				endpointCounts: {
					[endpointSeries.endpoint_id]: point.count,
				},
			});
		}
	}
	return Array.from(byMinute.values()).sort(
		(left, right) => left.minuteMs - right.minuteMs,
	);
}

export function buildTcpConnectionTooltipHtml({
	minuteMs,
	totalCount,
	selectedEndpoints,
	endpointCounts,
	palette,
	totalMarker,
}: {
	minuteMs: number;
	totalCount: number;
	selectedEndpoints: AdminNodeTcpConnectionsResponse["endpoints"];
	endpointCounts: Record<string, number>;
	palette: Pick<TooltipThemePalette, "border" | "foreground" | "muted">;
	totalMarker: string;
}): string {
	const endpointRows = selectedEndpoints
		.map((endpoint) => {
			const count = endpointCounts[endpoint.endpoint_id] ?? 0;
			return `
				<div style="display:flex;align-items:center;justify-content:space-between;gap:16px;margin-top:6px;">
					<span style="font-size:13px;color:${palette.muted};">${escapeHtml(endpointLabel(endpoint))}</span>
					<span style="font-size:13px;color:${palette.foreground};font-weight:600;white-space:nowrap;">${escapeHtml(formatConnectionCount(count))}</span>
				</div>
			`;
		})
		.join("");

	return `
		<div style="min-width:260px;">
			<div style="font-size:14px;color:${palette.muted};font-weight:400;line-height:1.2;">
				${escapeHtml(formatTooltipDateTime(minuteMs))}
			</div>
			<div style="display:flex;align-items:center;justify-content:space-between;gap:16px;margin-top:10px;">
				<span style="display:flex;align-items:center;gap:6px;font-size:13px;color:${palette.muted};">
					${totalMarker}
					<span>Total</span>
				</span>
				<span style="font-size:14px;color:${palette.foreground};font-weight:800;white-space:nowrap;">
					${escapeHtml(formatConnectionCount(totalCount))}
				</span>
			</div>
			<div style="margin-top:8px;padding-top:8px;border-top:1px solid ${palette.border};">
				${endpointRows}
			</div>
		</div>
	`;
}

function shouldRenderECharts(): boolean {
	if (typeof navigator === "undefined") return true;
	return !navigator.userAgent.toLowerCase().includes("jsdom");
}

function resolveCssColor(value: string, fallback: string): string {
	if (typeof document === "undefined" || !document.body) return fallback;
	const probe = document.createElement("span");
	probe.style.color = value;
	if (!probe.style.color) return fallback;
	probe.style.opacity = "0";
	probe.style.pointerEvents = "none";
	probe.style.position = "fixed";
	document.body.append(probe);
	const resolved = getComputedStyle(probe).color;
	probe.remove();
	return resolved || fallback;
}

function resolveChartLineColor(): string {
	if (
		typeof window === "undefined" ||
		typeof document === "undefined" ||
		typeof window.getComputedStyle !== "function"
	) {
		return CHART_COLOR_FALLBACK;
	}
	return (
		window
			.getComputedStyle(document.documentElement)
			.getPropertyValue("--primary")
			.trim() || CHART_COLOR_FALLBACK
	);
}

function resolveTooltipPalette(): TooltipThemePalette {
	return {
		background: resolveCssColor("var(--popover)", TOOLTIP_BACKGROUND_FALLBACK),
		border: resolveCssColor("var(--border)", TOOLTIP_BORDER_FALLBACK),
		foreground: resolveCssColor(
			"var(--popover-foreground)",
			TOOLTIP_FOREGROUND_FALLBACK,
		),
		muted: resolveCssColor("var(--muted-foreground)", TOOLTIP_MUTED_FALLBACK),
		shadow: resolveCssColor("var(--xp-overlay)", TOOLTIP_SHADOW_FALLBACK),
	};
}

function safeTimestamp(value: string | null | undefined): number | null {
	if (!value) return null;
	const timestamp = new Date(value).getTime();
	return Number.isNaN(timestamp) ? null : timestamp;
}

function markerSwatch(color: string): string {
	return `<span style="display:inline-block;margin-right:0;border-radius:10px;width:10px;height:10px;background-color:${color};"></span>`;
}

function TooltipPreviewCard({
	data,
	palette,
	markerColor,
}: {
	data: TooltipPreviewData;
	palette: TooltipThemePalette;
	markerColor: string;
}) {
	return (
		<div
			className="pointer-events-none absolute right-6 top-6 z-10 max-w-[min(20rem,calc(100%-3rem))] rounded-2xl border p-3 shadow-lg"
			style={{
				backgroundColor: palette.background,
				borderColor: palette.border,
				boxShadow: `0 10px 18px 0 ${palette.shadow}`,
			}}
		>
			<div
				className="text-sm"
				style={{ color: palette.muted, lineHeight: 1.2 }}
			>
				{formatTooltipDateTime(data.minuteMs)}
			</div>
			<div className="mt-2.5 flex items-center justify-between gap-4">
				<span
					className="flex items-center gap-1.5 text-[13px]"
					style={{ color: palette.muted }}
				>
					<span
						className="inline-block h-2.5 w-2.5 rounded-full"
						style={{ backgroundColor: markerColor }}
					/>
					<span>Total</span>
				</span>
				<span
					className="whitespace-nowrap text-sm font-extrabold"
					style={{ color: palette.foreground }}
				>
					{formatConnectionCount(data.totalCount)}
				</span>
			</div>
			<div
				className="mt-2 border-t pt-2"
				style={{ borderColor: palette.border }}
			>
				{data.endpointRows.map((row) => (
					<div
						key={row.endpointId}
						className="mt-1.5 flex items-center justify-between gap-4"
					>
						<span className="text-[13px]" style={{ color: palette.muted }}>
							{row.label}
						</span>
						<span
							className="whitespace-nowrap text-[13px] font-semibold"
							style={{ color: palette.foreground }}
						>
							{formatConnectionCount(row.count)}
						</span>
					</div>
				))}
			</div>
		</div>
	);
}

function ChartSurface({
	ariaLabel,
	option,
	chartRef,
}: {
	ariaLabel: string;
	option: EChartsOption;
	chartRef?: React.RefObject<EChartsReactRef | null>;
}) {
	if (!shouldRenderECharts()) {
		return (
			<div
				role="img"
				aria-label={ariaLabel}
				className="w-full rounded-xl border border-border/70 bg-card/80"
				style={{ height: CHART_HEIGHT }}
			/>
		);
	}

	return (
		<ReactEChartsCore
			echarts={echarts}
			option={option}
			notMerge
			lazyUpdate
			autoResize
			ref={chartRef}
			opts={SVG_RENDERER}
			style={{ height: CHART_HEIGHT, width: "100%" }}
		/>
	);
}

export function TcpConnectionUsageView({
	window,
	onWindowChange,
	report,
	isFetching = false,
	tooltipPreviewMinute = null,
}: TcpConnectionUsageViewProps) {
	const endpointIds = useMemo(
		() => report.endpoints.map((endpoint) => endpoint.endpoint_id),
		[report.endpoints],
	);
	const [selectedEndpointIds, setSelectedEndpointIds] = useState<string[]>(
		computeDefaultSelection(report.endpoints),
	);

	useEffect(() => {
		setSelectedEndpointIds((current) => {
			if (
				current.length > 0 &&
				current.every((endpointId) => endpointIds.includes(endpointId))
			) {
				return current;
			}
			return computeDefaultSelection(report.endpoints);
		});
	}, [endpointIds, report.endpoints]);

	const selectedSet = useMemo(
		() => new Set(selectedEndpointIds),
		[selectedEndpointIds],
	);

	const selectedEndpoints = useMemo(
		() =>
			report.endpoints.filter((endpoint) =>
				selectedSet.has(endpoint.endpoint_id),
			),
		[report.endpoints, selectedSet],
	);
	const allSelected = selectedEndpointIds.length === endpointIds.length;
	const selectedSeries = useMemo(
		() =>
			report.per_endpoint_series.filter((series) =>
				selectedSet.has(series.endpoint_id),
			),
		[report.per_endpoint_series, selectedSet],
	);
	const minuteBreakdowns = useMemo(
		() => buildMinuteConnectionBreakdowns(selectedSeries),
		[selectedSeries],
	);
	const minuteBreakdownByMs = useMemo(
		() =>
			new Map(
				minuteBreakdowns.map((breakdown) => [breakdown.minuteMs, breakdown]),
			),
		[minuteBreakdowns],
	);
	const chartRef = useRef<EChartsReactRef | null>(null);

	const aggregatedSeries = useMemo(() => {
		return minuteBreakdowns.map((breakdown) => ({
			minute: breakdown.minute,
			count: breakdown.totalCount,
		}));
	}, [minuteBreakdowns]);

	const unsupportedWarning = report.warnings.find(
		(warning) => warning.code === "unsupported_platform",
	);
	const nonUnsupportedWarnings = report.warnings.filter(
		(warning) => warning.code !== "unsupported_platform",
	);
	const hasEndpoints = report.endpoints.length > 0;
	const hasSelection = selectedEndpointIds.length > 0;
	const currentCount = aggregatedSeries.at(-1)?.count ?? 0;
	const peakCount = Math.max(
		0,
		...aggregatedSeries.map((point) => point.count),
	);
	const chartLineColor = resolveChartLineColor();
	const tooltipPalette = resolveTooltipPalette();
	const compactTimeAxis = shouldCompactTimeAxis();
	const chartAriaLabel = hasSelection
		? `TCP connections per minute chart, current ${currentCount} connections, peak ${peakCount}, ${selectedEndpoints.length} endpoints selected`
		: "TCP connections per minute chart";

	function selectAllEndpoints() {
		setSelectedEndpointIds(computeDefaultSelection(report.endpoints));
	}

	function toggleEndpoint(endpointId: string, next: boolean) {
		setSelectedEndpointIds((current) => {
			if (next) {
				return current.includes(endpointId)
					? current
					: [...current, endpointId];
			}
			return current.filter((value) => value !== endpointId);
		});
	}

	function selectOnlyEndpoint(endpointId: string) {
		setSelectedEndpointIds([endpointId]);
	}

	const chartPoints = useMemo(
		() =>
			aggregatedSeries.map((point) => ({
				minuteMs: safeTimestamp(point.minute) ?? 0,
				count: point.count,
			})),
		[aggregatedSeries],
	);
	const previewMinuteMs = safeTimestamp(tooltipPreviewMinute);
	const previewBreakdown = useMemo(
		() =>
			previewMinuteMs === null
				? null
				: (minuteBreakdownByMs.get(previewMinuteMs) ?? null),
		[minuteBreakdownByMs, previewMinuteMs],
	);
	const previewTooltipData = useMemo<TooltipPreviewData | null>(() => {
		if (!previewBreakdown) return null;
		return {
			minuteMs: previewBreakdown.minuteMs,
			totalCount: previewBreakdown.totalCount,
			endpointRows: selectedEndpoints.map((endpoint) => ({
				endpointId: endpoint.endpoint_id,
				label: endpointLabel(endpoint),
				count: previewBreakdown.endpointCounts[endpoint.endpoint_id] ?? 0,
			})),
		};
	}, [previewBreakdown, selectedEndpoints]);

	const chartOption = useMemo<EChartsOption>(() => {
		const points = chartPoints.map((point) => [point.minuteMs, point.count]);
		const maxCount = Math.max(
			1,
			...aggregatedSeries.map((point) => point.count),
		);
		return {
			animationDuration: 220,
			grid: {
				top: 24,
				right: 18,
				bottom: 36,
				left: 42,
			},
			tooltip: {
				backgroundColor: tooltipPalette.background,
				borderColor: tooltipPalette.border,
				borderWidth: 1,
				padding: 12,
				shadowBlur: 18,
				shadowColor: tooltipPalette.shadow,
				shadowOffsetY: 10,
				extraCssText: "border-radius: 16px;",
				textStyle: {
					color: tooltipPalette.foreground,
				},
				trigger: "axis",
				formatter: (params: unknown) => {
					const axisParams = Array.isArray(params) ? params : [params];
					const firstParam = axisParams[0] as TooltipAxisParam | undefined;
					const minuteMs = coerceTooltipMinuteMs(firstParam?.axisValue);
					if (minuteMs === null) return "";
					const breakdown = minuteBreakdownByMs.get(minuteMs);
					if (!breakdown) {
						return escapeHtml(formatTooltipDateTime(minuteMs));
					}
					return buildTcpConnectionTooltipHtml({
						minuteMs,
						totalCount: breakdown.totalCount,
						selectedEndpoints,
						endpointCounts: breakdown.endpointCounts,
						palette: tooltipPalette,
						totalMarker: firstParam?.marker ?? markerSwatch(chartLineColor),
					});
				},
			},
			xAxis: {
				type: "time",
				axisLabel: {
					color: CHART_AXIS_COLOR,
					formatter: (value: number) => formatAxisTime(value, window),
					hideOverlap: true,
					margin: compactTimeAxis ? 14 : 8,
				},
				axisLine: {
					lineStyle: { color: CHART_GRID_COLOR },
				},
				axisTick: { show: false },
				splitLine: { show: false },
				splitNumber: compactTimeAxis ? 4 : window === "24h" ? 6 : 7,
			},
			yAxis: {
				type: "value",
				axisLabel: {
					color: CHART_AXIS_COLOR,
				},
				axisLine: { show: false },
				axisTick: { show: false },
				minInterval: 1,
				max: maxCount,
				splitLine: {
					lineStyle: {
						color: CHART_GRID_COLOR,
					},
				},
				splitNumber: 4,
			},
			series: [
				{
					type: "line",
					smooth: false,
					showSymbol: false,
					emphasis: {
						disabled: true,
					},
					data: points,
					lineStyle: {
						width: 2,
						color: chartLineColor,
					},
					areaStyle: {
						color: chartLineColor,
						opacity: 0.18,
					},
				},
			],
		};
	}, [
		aggregatedSeries,
		chartLineColor,
		chartPoints,
		compactTimeAxis,
		minuteBreakdownByMs,
		selectedEndpoints,
		tooltipPalette,
		window,
	]);

	if (unsupportedWarning) {
		return (
			<div className="space-y-4">
				<div className={alertClass("warning", "py-2")}>
					<span>{unsupportedWarning.message}</span>
				</div>
				{nonUnsupportedWarnings.map((warning) => (
					<div key={warning.code} className={alertClass("info", "py-2")}>
						<span>{warning.message}</span>
					</div>
				))}
				<PageState
					variant="empty"
					title="TCP connection count is unavailable"
					description="This node cannot expose Linux socket-level TCP connection counts on the current platform."
				/>
			</div>
		);
	}

	if (!hasEndpoints) {
		return (
			<PageState
				variant="empty"
				title="No business endpoints"
				description="This node has no business inbound endpoints to sample for TCP connection counts."
			/>
		);
	}

	return (
		<div className="space-y-4">
			<div className="flex flex-wrap items-start justify-between gap-3">
				<div>
					<h2 className="xp-card-title">TCP connection count</h2>
					<p className="text-sm text-muted-foreground">
						Per-minute ESTABLISHED inbound TCP connections on this node&apos;s
						business endpoint ports.
					</p>
					<p className="text-sm text-muted-foreground">
						Window: {formatDateTime(report.window_start)} {"->"}{" "}
						{formatDateTime(report.window_end)}
					</p>
				</div>
				<div className="flex flex-wrap items-center gap-2">
					{(["24h", "7d"] as const).map((option) => (
						<Button
							key={option}
							type="button"
							variant={window === option ? "secondary" : "ghost"}
							size="sm"
							onClick={() => onWindowChange(option)}
						>
							{option}
						</Button>
					))}
				</div>
			</div>

			{nonUnsupportedWarnings.map((warning) => (
				<div key={warning.code} className={alertClass("info", "py-2")}>
					<span>{warning.message}</span>
				</div>
			))}

			<div className="space-y-4 border-t border-border/70 pt-4">
				<div className="flex flex-wrap items-start justify-between gap-3">
					<div className="space-y-1">
						<p className="text-lg font-semibold">Connections per minute</p>
						<p className="text-sm text-muted-foreground">
							Combined across selected endpoints. Shared sessions are not
							deduplicated.
						</p>
					</div>
					<div className="flex flex-wrap items-center gap-2">
						<Badge variant="ghost" size="sm" className="font-normal">
							<span className="font-semibold text-foreground">
								{selectedEndpoints.length}
							</span>
							visible
						</Badge>
						<Badge variant="ghost" size="sm" className="font-normal">
							<span className="font-semibold text-foreground">
								{currentCount}
							</span>
							current
						</Badge>
						<Badge variant="ghost" size="sm" className="font-normal">
							<span className="font-semibold text-foreground">{peakCount}</span>
							peak
						</Badge>
						{isFetching ? (
							<Badge variant="outline" size="sm" className="font-normal">
								Refreshing
							</Badge>
						) : null}
					</div>
				</div>

				<div>
					{!hasSelection ? (
						<PageState
							variant="empty"
							title="No endpoints selected"
							description="Select at least one endpoint to inspect the combined TCP connection history."
							action={
								<Button
									variant="secondary"
									size="sm"
									onClick={selectAllEndpoints}
								>
									Select all endpoints
								</Button>
							}
						/>
					) : aggregatedSeries.length === 0 ? (
						<PageState
							variant="empty"
							title="No TCP connection history yet"
							description="This window has no sampled TCP connection data for the selected endpoints."
						/>
					) : (
						<div className="relative">
							<ChartSurface
								ariaLabel={chartAriaLabel}
								option={chartOption}
								chartRef={chartRef}
							/>
							{previewTooltipData ? (
								<TooltipPreviewCard
									data={previewTooltipData}
									palette={tooltipPalette}
									markerColor={chartLineColor}
								/>
							) : null}
						</div>
					)}
				</div>

				<div className="border-t border-border/70 pt-4">
					<div className="flex flex-wrap items-center gap-3">
						{report.endpoints.map((endpoint) => {
							const checked = selectedSet.has(endpoint.endpoint_id);
							const canSelectOnly =
								hasSelection &&
								(selectedEndpointIds.length > 1 ||
									selectedEndpointIds[0] !== endpoint.endpoint_id);
							return (
								<div
									key={endpoint.endpoint_id}
									className={cn(
										"inline-flex w-full items-center gap-2 rounded-xl border border-border/70 bg-muted/25 px-2.5 py-2 text-sm sm:w-auto sm:px-3",
										checked ? "ring-1 ring-primary/40" : "",
									)}
								>
									<Checkbox
										id={`tcp-endpoint-${endpoint.endpoint_id}`}
										checked={checked}
										onCheckedChange={(next) =>
											toggleEndpoint(endpoint.endpoint_id, Boolean(next))
										}
									/>
									<label
										htmlFor={`tcp-endpoint-${endpoint.endpoint_id}`}
										className="min-w-0 flex-1 cursor-pointer pr-1"
									>
										{endpointLabel(endpoint)}
									</label>
									{canSelectOnly ? (
										<Button
											variant="ghost"
											size="sm"
											className="min-h-11 rounded-lg px-3 text-xs text-muted-foreground hover:text-foreground sm:min-h-8"
											onClick={() => selectOnlyEndpoint(endpoint.endpoint_id)}
										>
											Only
										</Button>
									) : null}
								</div>
							);
						})}
						{hasSelection && !allSelected ? (
							<Button
								variant="ghost"
								size="sm"
								className="min-h-11 rounded-lg px-3 text-xs text-muted-foreground hover:text-foreground sm:min-h-8"
								onClick={selectAllEndpoints}
							>
								Reset all
							</Button>
						) : null}
					</div>
				</div>
			</div>
		</div>
	);
}
