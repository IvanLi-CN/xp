import { useId } from "react";

import type {
	AdminIpUsageListEntry,
	AdminIpUsageTimelineLane,
	AdminIpUsageWarning,
	AdminIpUsageWindow,
	AdminNodeIpUsageResponse,
	AdminUserIpUsageNodeGroup,
} from "../api/adminIpUsage";

const CHART_HEIGHT = 220;

type SharedIpUsageReport = Pick<
	AdminNodeIpUsageResponse,
	| "window_start"
	| "window_end"
	| "warnings"
	| "unique_ip_series"
	| "timeline"
	| "ips"
>;

type IpUsageViewProps = {
	title: string;
	description: string;
	window: AdminIpUsageWindow;
	onWindowChange: (window: AdminIpUsageWindow) => void;
	report: SharedIpUsageReport;
	isFetching?: boolean;
	emptyTitle?: string;
};

function formatDateTime(value: string): string {
	const dt = new Date(value);
	if (Number.isNaN(dt.getTime())) return value;
	return dt.toLocaleString();
}

function formatShortTime(value: string): string {
	const dt = new Date(value);
	if (Number.isNaN(dt.getTime())) return value;
	return dt.toLocaleString([], {
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
	});
}

function buildAreaPath(
	points: Array<[number, number]>,
	height: number,
): string {
	if (points.length === 0) return "";
	const line = points
		.map(
			([x, y], index) =>
				`${index === 0 ? "M" : "L"}${x.toFixed(2)},${y.toFixed(2)}`,
		)
		.join(" ");
	const firstX = points[0]?.[0] ?? 0;
	const lastX = points[points.length - 1]?.[0] ?? 0;
	return `${line} L${lastX.toFixed(2)},${height.toFixed(2)} L${firstX.toFixed(2)},${height.toFixed(2)} Z`;
}

function isReportEmpty(report: SharedIpUsageReport): boolean {
	return report.ips.length === 0 && report.timeline.length === 0;
}

function hasWarning(
	warnings: AdminIpUsageWarning[],
	code: "online_stats_unavailable" | "geo_db_missing",
): boolean {
	return warnings.some((warning) => warning.code === code);
}

function WindowSwitch({
	window,
	onWindowChange,
}: Pick<IpUsageViewProps, "window" | "onWindowChange">) {
	return (
		<div className="join">
			{(["24h", "7d"] as const).map((option) => (
				<button
					key={option}
					type="button"
					className={`btn join-item btn-sm ${window === option ? "btn-primary" : "btn-ghost"}`}
					onClick={() => onWindowChange(option)}
				>
					{option}
				</button>
			))}
		</div>
	);
}

function UniqueIpAreaChart({ report }: { report: SharedIpUsageReport }) {
	const chartId = useId().replace(/:/g, "");
	const gradientId = `${chartId}-gradient`;
	const titleId = `${chartId}-title`;
	const series = report.unique_ip_series;
	const maxCount = Math.max(1, ...series.map((point) => point.count));
	const width = Math.max(1, series.length - 1);
	const points = series.map((point, index) => {
		const x = width === 0 ? 0 : (index / width) * 100;
		const y =
			CHART_HEIGHT - (point.count / maxCount) * (CHART_HEIGHT - 20) - 10;
		return [x, y] as [number, number];
	});
	const areaPath = buildAreaPath(points, CHART_HEIGHT);
	const linePath = points
		.map(
			([x, y], index) =>
				`${index === 0 ? "M" : "L"}${x.toFixed(2)},${y.toFixed(2)}`,
		)
		.join(" ");
	const ticks = [0, Math.ceil(maxCount / 2), maxCount].filter(
		(value, index, array) => array.indexOf(value) === index,
	);

	return (
		<div className="rounded-2xl border border-base-300 bg-base-200/60 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">Unique IPs per minute</p>
					<p className="text-xs opacity-70">
						Exact minute series without downsampling.
					</p>
				</div>
				<div className="badge badge-outline">max {maxCount}</div>
			</div>
			<div className="relative overflow-hidden rounded-xl bg-gradient-to-br from-primary/8 via-base-100 to-secondary/10">
				<svg
					viewBox={`0 0 100 ${CHART_HEIGHT}`}
					className="h-56 w-full"
					role="img"
					aria-labelledby={titleId}
				>
					<title id={titleId}>Unique IPs per minute</title>
					{ticks.map((tick) => {
						const y =
							CHART_HEIGHT - (tick / maxCount) * (CHART_HEIGHT - 20) - 10;
						return (
							<g key={tick}>
								<line
									x1="0"
									y1={y}
									x2="100"
									y2={y}
									stroke="currentColor"
									strokeOpacity="0.12"
								/>
								<text
									x="1"
									y={Math.max(10, y - 2)}
									fontSize="3.2"
									fill="currentColor"
									fillOpacity="0.65"
								>
									{tick}
								</text>
							</g>
						);
					})}
					<path d={areaPath} fill={`url(#${gradientId})`} opacity="0.9" />
					<path
						d={linePath}
						fill="none"
						stroke="currentColor"
						strokeWidth="1.6"
						strokeLinecap="round"
						strokeLinejoin="round"
					/>
					<defs>
						<linearGradient id={gradientId} x1="0" x2="0" y1="0" y2="1">
							<stop
								offset="0%"
								stopColor="oklch(var(--p))"
								stopOpacity="0.65"
							/>
							<stop
								offset="100%"
								stopColor="oklch(var(--p))"
								stopOpacity="0.08"
							/>
						</linearGradient>
					</defs>
				</svg>
			</div>
			<div className="mt-3 flex items-center justify-between text-xs opacity-70">
				<span>{formatShortTime(report.window_start)}</span>
				<span>{formatShortTime(report.window_end)}</span>
			</div>
		</div>
	);
}

function TimelineChart({
	lanes,
	window,
	windowStart,
}: {
	lanes: AdminIpUsageTimelineLane[];
	window: AdminIpUsageWindow;
	windowStart: string;
}) {
	const totalMinutes = window === "24h" ? 24 * 60 : 7 * 24 * 60;
	const baseline = new Date(windowStart);

	return (
		<div className="rounded-2xl border border-base-300 bg-base-200/60 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">IP occupancy lanes</p>
					<p className="text-xs opacity-70">
						Top 20 endpoint/IP rows merged into continuous minute spans.
					</p>
				</div>
				<div className="badge badge-outline">{lanes.length} lanes</div>
			</div>
			{lanes.length === 0 ? (
				<div className="rounded-xl border border-dashed border-base-300 px-4 py-8 text-center text-sm opacity-70">
					No occupancy lanes in this window.
				</div>
			) : (
				<div className="space-y-3">
					{lanes.map((lane) => (
						<div
							key={lane.lane_key}
							className="grid gap-2 md:grid-cols-[18rem,minmax(0,1fr)] md:items-center"
						>
							<div className="min-w-0">
								<p className="truncate text-sm font-medium">{lane.ip}</p>
								<p className="truncate text-xs opacity-70">
									{lane.endpoint_tag} · {lane.minutes} min
								</p>
							</div>
							<div className="relative h-11 rounded-xl border border-base-300 bg-base-100/80">
								<div className="absolute inset-x-0 top-1/2 h-px -translate-y-1/2 bg-base-300/80" />
								{lane.segments.map((segment, index) => {
									const start = new Date(segment.start_minute);
									const end = new Date(segment.end_minute);
									if (
										Number.isNaN(start.getTime()) ||
										Number.isNaN(end.getTime())
									) {
										return null;
									}
									const safeBaseline = !Number.isNaN(baseline.getTime())
										? baseline
										: start;
									const offsetMinutes = Math.max(
										0,
										Math.floor(
											(start.getTime() - safeBaseline.getTime()) / 60000,
										),
									);
									const segmentMinutes = Math.max(
										1,
										Math.floor((end.getTime() - start.getTime()) / 60000) + 1,
									);
									const left = (offsetMinutes / totalMinutes) * 100;
									const width = Math.max(
										(segmentMinutes / totalMinutes) * 100,
										0.35,
									);
									return (
										<div
											key={`${lane.lane_key}-${segment.start_minute}-${index}`}
											className="absolute top-1/2 h-5 -translate-y-1/2 rounded-full bg-gradient-to-r from-primary to-secondary shadow-sm"
											style={{ left: `${left}%`, width: `${width}%` }}
											title={`${formatDateTime(segment.start_minute)} → ${formatDateTime(segment.end_minute)}`}
										/>
									);
								})}
							</div>
						</div>
					))}
				</div>
			)}
		</div>
	);
}

function IpListTable({ ips }: { ips: AdminIpUsageListEntry[] }) {
	return (
		<div className="rounded-2xl border border-base-300 bg-base-200/60 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div>
					<p className="text-sm font-semibold">IP list</p>
					<p className="text-xs opacity-70">
						Aggregated by IP with unique minutes inside the selected window.
					</p>
				</div>
				<div className="badge badge-outline">{ips.length} IPs</div>
			</div>
			<div className="overflow-x-auto rounded-xl border border-base-300 bg-base-100">
				<table className="table table-sm">
					<thead>
						<tr>
							<th>IP</th>
							<th>Minutes</th>
							<th>Endpoints</th>
							<th>Region</th>
							<th>Operator</th>
							<th>Last seen</th>
						</tr>
					</thead>
					<tbody>
						{ips.length === 0 ? (
							<tr>
								<td colSpan={6} className="py-8 text-center text-sm opacity-70">
									No IP rows in this window.
								</td>
							</tr>
						) : (
							ips.map((entry) => (
								<tr key={entry.ip}>
									<td className="font-mono text-xs">{entry.ip}</td>
									<td>{entry.minutes}</td>
									<td className="text-xs">
										{entry.endpoint_tags.join(", ") || "-"}
									</td>
									<td className="text-xs">{entry.region || "Unknown"}</td>
									<td className="text-xs">{entry.operator || "Unknown"}</td>
									<td className="text-xs">
										{formatDateTime(entry.last_seen_at)}
									</td>
								</tr>
							))
						)}
					</tbody>
				</table>
			</div>
		</div>
	);
}

function WarningList({ warnings }: { warnings: AdminIpUsageWarning[] }) {
	if (warnings.length === 0) return null;
	return (
		<div className="space-y-2">
			{warnings.map((warning) => (
				<div
					key={warning.code}
					className={`alert py-2 text-sm ${warning.code === "online_stats_unavailable" ? "alert-warning" : "alert-info"}`}
				>
					<span>{warning.message}</span>
				</div>
			))}
		</div>
	);
}

export function IpUsageView({
	title,
	description,
	window,
	onWindowChange,
	report,
	isFetching = false,
	emptyTitle = "No inbound IP data",
}: IpUsageViewProps) {
	const empty = isReportEmpty(report);
	const blockingWarning = hasWarning(
		report.warnings,
		"online_stats_unavailable",
	);

	return (
		<div className="card bg-base-100 shadow">
			<div className="card-body space-y-4">
				<div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
					<div>
						<h2 className="card-title">{title}</h2>
						<p className="text-sm opacity-70">{description}</p>
						<p className="mt-1 text-xs opacity-60">
							Window: {formatDateTime(report.window_start)} →{" "}
							{formatDateTime(report.window_end)}
						</p>
					</div>
					<div className="flex items-center gap-2">
						{isFetching ? (
							<span className="badge badge-ghost">refreshing</span>
						) : null}
						<WindowSwitch window={window} onWindowChange={onWindowChange} />
					</div>
				</div>

				<WarningList warnings={report.warnings} />

				{blockingWarning && empty ? (
					<div className="rounded-2xl border border-warning/40 bg-warning/10 px-4 py-10 text-center">
						<p className="text-base font-semibold">
							Online snapshots are unavailable
						</p>
						<p className="mt-2 text-sm opacity-80">
							This node is not exposing `statsUserOnline`, so xp cannot collect
							per-minute inbound IP occupancy.
						</p>
					</div>
				) : empty ? (
					<div className="rounded-2xl border border-dashed border-base-300 px-4 py-10 text-center">
						<p className="text-base font-semibold">{emptyTitle}</p>
						<p className="mt-2 text-sm opacity-70">
							No unique IPs were seen in the selected time window.
						</p>
					</div>
				) : (
					<div className="space-y-4">
						<UniqueIpAreaChart report={report} />
						<TimelineChart
							lanes={report.timeline}
							window={window}
							windowStart={report.window_start}
						/>
						<IpListTable ips={report.ips} />
					</div>
				)}
			</div>
		</div>
	);
}

export type IpUsageGroupReport = AdminUserIpUsageNodeGroup;
