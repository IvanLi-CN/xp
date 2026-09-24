import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { BackendApiError } from "../api/backendError";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { ResourcePeerDiagnosticState } from "./ResourcePeerDiagnosticState";
import {
	RESOURCE_HISTORY_CHARTS,
	RUNTIME_RESOURCE_HISTORY_CHARTS,
	buildResourceHistoryChartOption,
} from "./ResourceSnapshotPanel";
import type { EChartsThemePalette } from "./echarts-theme";

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

describe("ResourcePeerDiagnosticState", () => {
	const requestId = "01M0C1SJ5M1JWE6CCKMXNXPZ78";

	function diagnosticError(
		failure:
			| "circuit_open"
			| "pre_response_timeout"
			| "pre_response_transport"
			| "unsigned_response"
			| "acknowledgement_missing"
			| "acknowledgement_invalid"
			| "outcome_unknown",
		dispatch = "dispatched_no_verified_response",
	) {
		return new BackendApiError({
			status: 504,
			code: "resource_peer_unavailable",
			message: "resource snapshot is unavailable from the target node",
			details: {
				diagnostic: {
					origin: { node_id: "101", node_name: "101" },
					target: { node_id: "us", node_name: "us" },
					route_attempts: [
						{
							route: "direct_mesh",
							failure,
							acknowledgement: "not_observed",
							dispatch,
							observed_at: "2026-09-24T00:00:00Z",
							request_id: requestId,
							elapsed_ms: 100,
							retry_count: 0,
						},
					],
					public_circuit: "closed",
					request_id: requestId,
				},
			},
		});
	}

	function diagnosticProps(
		error: BackendApiError,
		onRetry: () => void = () => undefined,
		isOnline = true,
	) {
		return {
			error,
			isFetching: false,
			isOnline,
			onRetry,
		};
	}

	it("shows the source, target, route boundary, and copyable correlation id", () => {
		render(
			<ResourcePeerDiagnosticState
				error={
					new BackendApiError({
						status: 504,
						code: "resource_peer_unavailable",
						message: "resource snapshot is unavailable from the target node",
						details: {
							diagnostic: {
								origin: { node_id: "101", node_name: "101" },
								target: { node_id: "us", node_name: "us" },
								route_attempts: [
									{
										route: "direct_mesh",
										failure: "pre_response_timeout",
										acknowledgement: "not_observed",
										dispatch: "dispatched_no_verified_response",
										observed_at: "2026-09-24T00:00:00Z",
										request_id: "01M0C1SJ5M1JWE6CCKMXNXPZ78",
										elapsed_ms: 5000,
										retry_count: 0,
									},
									{
										route: "public",
										failure: "circuit_open",
										acknowledgement: "not_observed",
										dispatch: "not_dispatched",
										observed_at: "2026-09-24T00:00:05Z",
										request_id: "01M0C1SJ5M1JWE6CCKMXNXPZ78",
										elapsed_ms: 5000,
										retry_count: 0,
									},
								],
								public_circuit: "open",
								request_id: "01M0C1SJ5M1JWE6CCKMXNXPZ78",
							},
						},
					})
				}
				isFetching={false}
				isOnline
				onRetry={() => undefined}
			/>,
		);

		expect(screen.getByText("101 → us")).toBeVisible();
		expect(screen.getByText(/Public 备用路径未发送/)).toBeVisible();
		expect(screen.getByText("01M0C1SJ5M1JWE6CCKMXNXPZ78")).toBeVisible();
		expect(screen.getByRole("button", { name: "复制 ID" })).toBeVisible();
	});

	it.each([
		["pre_response_transport", /发生传输失败/],
		["unsigned_response", /没有可验证的签名确认/],
		["acknowledgement_missing", /响应缺少签名确认/],
		["acknowledgement_invalid", /签名确认无法验证/],
		["outcome_unknown", /请求结果未知/],
	] as const)("renders the %s boundary", (failure, expected) => {
		const view = render(
			<ResourcePeerDiagnosticState
				{...diagnosticProps(diagnosticError(failure))}
			/>,
		);

		expect(screen.getByText(expected)).toBeVisible();
		view.unmount();
	});

	it("copies the correlation id, retries, and disables retry offline", async () => {
		const writeText = vi.fn().mockResolvedValue(undefined);
		Object.defineProperty(navigator, "clipboard", {
			configurable: true,
			value: { writeText },
		});
		const onRetry = vi.fn();
		const view = render(
			<ResourcePeerDiagnosticState
				{...diagnosticProps(diagnosticError("pre_response_timeout"), onRetry)}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "复制 ID" }));
		await waitFor(() => expect(writeText).toHaveBeenCalledWith(requestId));
		await waitFor(() =>
			expect(screen.getByRole("button", { name: "已复制" })).toBeVisible(),
		);
		fireEvent.click(screen.getByRole("button", { name: "Retry" }));
		expect(onRetry).toHaveBeenCalledTimes(1);
		view.unmount();

		const offlineView = render(
			<ResourcePeerDiagnosticState
				{...diagnosticProps(
					diagnosticError("pre_response_timeout"),
					onRetry,
					false,
				)}
			/>,
		);
		expect(screen.getByRole("button", { name: "Retry" })).toBeDisabled();
		offlineView.unmount();
	});

	it("does not describe a Direct Mesh circuit as a Public circuit", () => {
		render(
			<ResourcePeerDiagnosticState
				error={
					new BackendApiError({
						status: 504,
						code: "resource_peer_unavailable",
						message: "resource snapshot is unavailable from the target node",
						details: {
							diagnostic: {
								origin: { node_id: "101", node_name: "101" },
								target: { node_id: "us", node_name: "us" },
								route_attempts: [
									{
										route: "direct_mesh",
										failure: "circuit_open",
										acknowledgement: "not_observed",
										dispatch: "not_dispatched",
										observed_at: "2026-09-24T00:00:00Z",
										request_id: "01M0C1SJ5M1JWE6CCKMXNXPZ78",
										elapsed_ms: 0,
										retry_count: 0,
									},
								],
								public_circuit: "closed",
								request_id: "01M0C1SJ5M1JWE6CCKMXNXPZ78",
							},
						},
					})
				}
				isFetching={false}
				isOnline
				onRetry={() => undefined}
			/>,
		);

		expect(screen.getByText(/Direct Mesh 断路器当前处于冷却中/)).toBeVisible();
		expect(screen.queryByText(/Public 备用路径未发送/)).not.toBeInTheDocument();
	});
});
