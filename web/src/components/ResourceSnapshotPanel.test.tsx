import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { BackendApiError, throwIfNotOk } from "../api/backendError";
import { fixtureCatalog } from "../fixture-policy/catalog";
import * as resourceMonitoringFixtures from "../fixture-policy/resourceMonitoring";
import {
	RESOURCE_HISTORY_CHARTS,
	RUNTIME_RESOURCE_HISTORY_CHARTS,
	ResourceTabContent,
	buildResourceHistoryChartOption,
} from "./ResourceSnapshotPanel";
import type { EChartsThemePalette } from "./echarts-theme";

const supportedSnapshot = resourceMonitoringFixtures.supportedResourceSnapshot;

const resourceHistoryBase = Date.parse(
	fixtureCatalog.timestamp.t20260901T000000(),
);

function catalogTimestampOffset(index: number): string {
	return new Date(resourceHistoryBase + index * 15_000).toISOString();
}

const palette: EChartsThemePalette = {
	axis: "axis",
	axisPointer: "axisPointer",
	currentArea: "currentArea",
	currentDay: "currentDay",
	grid: "grid",
	primary: "primary",
	reference: "reference",
	tooltip: {
		background: "tooltipBackground",
		border: "tooltipBorder",
		foreground: "tooltipForeground",
		muted: "tooltipMuted",
		shadow: "tooltipShadow",
		shadowBlur: 12,
		shadowOffsetX: 0,
		shadowOffsetY: 4,
	},
};

describe("buildResourceHistoryChartOption", () => {
	it("renders each fixed resource series with the TCP history chart contract", () => {
		const points = [
			{
				observed_at: catalogTimestampOffset(0),
				value: fixtureCatalog.number.value20(),
			},
			{
				observed_at: catalogTimestampOffset(1),
				value: null,
			},
			{
				observed_at: catalogTimestampOffset(2),
				value: fixtureCatalog.number.value50(),
			},
		];

		for (const chart of RESOURCE_HISTORY_CHARTS) {
			const option = buildResourceHistoryChartOption(
				Object.fromEntries([[chart.series[0].metric, points]]),
				palette,
				chart,
			);

			expect(option).toMatchObject({
				grid: { top: 24, right: 18, bottom: 36, left: 42 },
				xAxis: {
					type: "time",
					min: 1_788_220_800_000,
					max: 1_788_220_830_000,
				},
				yAxis: {
					type: "value",
					min: 0,
					axisLabel: { formatter: expect.any(Function) },
				},
				series: [
					{
						name: chart.series[0].name,
						type: "line",
						data: [
							[1_788_220_800_000, 20],
							[1_788_220_815_000, null],
							[1_788_220_830_000, 50],
						],
						step: "end",
						smooth: false,
						connectNulls: false,
						showSymbol: false,
						lineStyle: { width: 2, color: "primary", join: "round" },
						areaStyle: { color: "primary", opacity: 0.18 },
					},
				],
			});

			if (chart.unit === "percent") {
				expect(option.yAxis).toMatchObject({ max: 100 });
			} else {
				expect(option.yAxis).not.toMatchObject({ max: 100 });
			}
		}
	});
});

describe("ResourceTabContent diagnostics", () => {
	it("uses the authoritative Retry-After header for the disabled retry state", async () => {
		const response = new Response(
			JSON.stringify({
				error: {
					code: "peer_circuit_open",
					message: "cooldown",
					details: {
						failure_layer: "circuit_breaker",
						retryable: true,
						retry_after_seconds: 1,
					},
				},
			}),
			{ status: 503, headers: { "Retry-After": "45" } },
		);
		let parsedError: unknown;
		try {
			await throwIfNotOk(response);
		} catch (error) {
			parsedError = error;
		}

		render(
			<ResourceTabContent
				capabilityUnavailable={false}
				isLoading={false}
				isError
				error={parsedError}
				isFetching={false}
				isOnline
				onRetry={() => undefined}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
				selectedRuntimeRole={null}
				onRuntimeDetailsChange={() => undefined}
			/>,
		);

		await waitFor(() =>
			expect(
				screen.getByRole("button", { name: "Retry resource read" }),
			).toHaveTextContent(/Retry in 4\ds/),
		);
		expect(
			screen.getByRole("button", { name: "Retry resource read" }),
		).toBeDisabled();
	});

	it("does not use raw backend text and disables retry during cooldown", () => {
		const onRetry = vi.fn();
		render(
			<ResourceTabContent
				capabilityUnavailable={false}
				isLoading={false}
				isError
				error={
					new BackendApiError({
						status: 503,
						code: "peer_circuit_open",
						message: "raw mesh details must stay hidden",
						details: {
							failure_layer: "circuit_breaker",
							retryable: true,
							retry_after_seconds: 30,
						},
					})
				}
				isFetching={false}
				isOnline
				onRetry={onRetry}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
				selectedRuntimeRole={null}
				onRuntimeDetailsChange={() => undefined}
			/>,
		);

		expect(screen.getByText("Peer path is cooling down")).toBeInTheDocument();
		expect(
			screen.queryByText(/raw mesh details must stay hidden/),
		).not.toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Retry resource read" }),
		).toBeDisabled();
	});

	it("keeps the cooldown deadline when the panel remounts", () => {
		vi.useFakeTimers();
		try {
			vi.setSystemTime(new Date("2026-09-24T12:00:00.000Z"));
			const error = new BackendApiError({
				status: 503,
				code: "peer_circuit_open",
				message: "cooldown",
				details: {
					failure_layer: "circuit_breaker",
					retryable: true,
					retry_after_seconds: 45,
				},
			});
			const props = {
				capabilityUnavailable: false,
				isLoading: false,
				isError: true,
				error,
				isFetching: false,
				isOnline: true,
				onRetry: () => undefined,
				historyByMetric: {},
				runtimeHistoryByMetric: {},
				selectedRuntimeRole: null,
				onRuntimeDetailsChange: () => undefined,
			};

			const view = render(<ResourceTabContent {...props} />);
			expect(
				screen.getByRole("button", { name: "Retry resource read" }),
			).toHaveTextContent("Retry in 45s");
			act(() => vi.advanceTimersByTime(10_000));
			view.unmount();
			render(<ResourceTabContent {...props} />);

			expect(
				screen.getByRole("button", { name: "Retry resource read" }),
			).toHaveTextContent("Retry in 35s");
		} finally {
			vi.useRealTimers();
		}
	});

	it("retains stale snapshots with distinct read and age labels", () => {
		render(
			<ResourceTabContent
				capabilityUnavailable={false}
				isLoading={false}
				isError
				error={
					new BackendApiError({
						status: 504,
						code: "peer_transport_timeout",
						message: "raw timeout details must stay hidden",
						details: { failure_layer: "peer_transport", retryable: true },
					})
				}
				isFetching={false}
				isOnline
				onRetry={() => undefined}
				dataUpdatedAt={Date.now() - 2 * 60 * 60 * 1000}
				snapshot={supportedSnapshot}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
				selectedRuntimeRole={null}
				onRuntimeDetailsChange={() => undefined}
			/>,
		);

		expect(
			screen.getByText(/Showing the last successful snapshot/),
		).toBeInTheDocument();
		expect(screen.getByText(/last successful read/)).toBeInTheDocument();
		expect(screen.getByText(/age 2 hr/)).toBeInTheDocument();
		expect(
			screen.queryByText(/raw timeout details must stay hidden/),
		).not.toBeInTheDocument();
	});

	it("updates stale age while the failed snapshot remains visible", () => {
		vi.useFakeTimers();
		const now = new Date("2026-09-24T12:00:00.000Z").getTime();
		vi.setSystemTime(now);

		render(
			<ResourceTabContent
				capabilityUnavailable={false}
				isLoading={false}
				isError
				error={
					new BackendApiError({
						status: 504,
						code: "peer_transport_timeout",
						message: "timeout details must stay hidden",
						details: {
							failure_layer: "peer_transport",
							retryable: true,
						},
					})
				}
				isFetching={false}
				isOnline
				onRetry={() => undefined}
				dataUpdatedAt={now - 59 * 60 * 1000}
				snapshot={supportedSnapshot}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
				selectedRuntimeRole={null}
				onRuntimeDetailsChange={() => undefined}
			/>,
		);

		expect(screen.getByText(/age 59 min/)).toBeInTheDocument();
		act(() => {
			vi.advanceTimersByTime(60_000);
		});
		expect(screen.getByText(/age 1 hr/)).toBeInTheDocument();
		vi.useRealTimers();
	});

	it("keeps chart points while showing a scoped history refresh failure", () => {
		render(
			<ResourceTabContent
				capabilityUnavailable={false}
				isLoading={false}
				isError={false}
				error={null}
				isFetching={false}
				isOnline
				onRetry={() => undefined}
				snapshot={supportedSnapshot}
				historyByMetric={{
					cpu_busy_percent: [
						{
							observed_at: catalogTimestampOffset(0),
							value: fixtureCatalog.number.value20(),
						},
					],
				}}
				historyErrorByMetric={{
					cpu_busy_percent: new BackendApiError({
						status: 504,
						code: "peer_transport_timeout",
						message: "raw history details must stay hidden",
						details: { failure_layer: "peer_transport", retryable: true },
					}),
				}}
				runtimeHistoryByMetric={{}}
				selectedRuntimeRole={null}
				onRuntimeDetailsChange={() => undefined}
			/>,
		);

		expect(
			screen.getByText("Refresh failed; showing the last successful points."),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Retry history" })).toBeEnabled();
		expect(
			screen.queryByText(/raw history details must stay hidden/),
		).not.toBeInTheDocument();
	});

	it("does not offer retry for unsupported remote history", () => {
		render(
			<ResourceTabContent
				capabilityUnavailable={false}
				isLoading={false}
				isError={false}
				error={null}
				isFetching={false}
				isOnline
				onRetry={() => undefined}
				snapshot={supportedSnapshot}
				historyByMetric={{}}
				historyErrorByMetric={{
					cpu_busy_percent: new BackendApiError({
						status: 501,
						code: "resource_monitoring_unsupported",
						message: "unsupported",
						details: { retryable: false },
					}),
				}}
				runtimeHistoryByMetric={{}}
				selectedRuntimeRole={null}
				onRuntimeDetailsChange={() => undefined}
			/>,
		);

		expect(
			screen.getAllByText(
				"The target node does not expose resource monitoring. No retry is scheduled.",
			).length,
		).toBeGreaterThan(0);
		expect(
			screen.queryByRole("button", { name: "Retry history" }),
		).not.toBeInTheDocument();
	});
});

describe("runtime resource charts", () => {
	it("keeps the runtime detail metrics fixed while grouping related series", () => {
		expect(
			RUNTIME_RESOURCE_HISTORY_CHARTS.map((chart) =>
				chart.series.map((series) => series.metric),
			),
		).toEqual([
			["cpu_percent"],
			["rss_bytes", "pss_bytes"],
			["read_bytes_per_second", "write_bytes_per_second"],
			["fd_count"],
			["thread_count"],
		]);
	});

	it("renders paired runtime metrics with the existing blue comparison-series style", () => {
		const chart = RUNTIME_RESOURCE_HISTORY_CHARTS.find(
			(candidate) => candidate.key === "memory",
		);
		expect(chart).toBeDefined();
		if (!chart) return;

		const option = buildResourceHistoryChartOption(
			{
				rss_bytes: [
					{
						observed_at: fixtureCatalog.timestamp.t20260901T000000(),
						value: fixtureCatalog.number.value100(),
					},
				],
				pss_bytes: [
					{
						observed_at: fixtureCatalog.timestamp.t20260901T000000(),
						value: fixtureCatalog.number.value60(),
					},
				],
			},
			palette,
			chart,
		);

		expect(option.series).toMatchObject([
			{
				name: "RSS",
				lineStyle: { width: 2, color: "primary", join: "round" },
				areaStyle: { color: "primary", opacity: 0.18 },
			},
			{
				name: "PSS",
				lineStyle: { width: 2, color: "#3478c6", join: "round" },
			},
		]);
	});
});
