import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type { AdminNodeTcpConnectionsResponse } from "../api/adminTcpConnections";
import {
	TcpConnectionUsageView,
	buildMinuteConnectionBreakdowns,
	buildTcpConnectionTooltipHtml,
} from "./TcpConnectionUsageView";

const baseReport: Pick<
	AdminNodeTcpConnectionsResponse,
	| "window_start"
	| "window_end"
	| "warnings"
	| "endpoints"
	| "per_endpoint_series"
> = {
	window_start: fixtureCatalog.slotString.s47(),
	window_end: fixtureCatalog.slotString.s48(),
	warnings: [],
	endpoints: [
		{
			endpoint_id: fixtureCatalog.slotString.s287(),
			endpoint_tag: fixtureCatalog.slotString.s280(),
			port: 443,
		},
		{
			endpoint_id: fixtureCatalog.slotString.s288(),
			endpoint_tag: fixtureCatalog.slotString.s289(),
			port: 8443,
		},
	],
	per_endpoint_series: [
		{
			endpoint_id: fixtureCatalog.slotString.s287(),
			endpoint_tag: fixtureCatalog.slotString.s280(),
			port: 443,
			series: [
				{ minute: fixtureCatalog.slotString.s52(), count: 2 },
				{ minute: fixtureCatalog.slotString.s48(), count: 4 },
			],
		},
		{
			endpoint_id: fixtureCatalog.slotString.s288(),
			endpoint_tag: fixtureCatalog.slotString.s289(),
			port: 8443,
			series: [
				{ minute: fixtureCatalog.slotString.s52(), count: 1 },
				{ minute: fixtureCatalog.slotString.s48(), count: 3 },
			],
		},
	],
};

const endpointALabel = `${fixtureCatalog.slotString.s280()} :443`;
const endpointBLabel = `${fixtureCatalog.slotString.s289()} :8443`;

describe("<TcpConnectionUsageView />", () => {
	it("builds tooltip content with total and per-endpoint counts", () => {
		const selectedSeries = baseReport.per_endpoint_series;
		const minuteBreakdowns = buildMinuteConnectionBreakdowns(selectedSeries);
		const latestBreakdown = minuteBreakdowns.at(-1);
		expect(latestBreakdown).toBeDefined();
		if (!latestBreakdown) {
			throw new Error("Expected latest breakdown to exist");
		}

		const tooltipHtml = buildTcpConnectionTooltipHtml({
			minuteMs: latestBreakdown.minuteMs,
			totalCount: latestBreakdown.totalCount,
			selectedEndpoints: baseReport.endpoints,
			endpointCounts: latestBreakdown.endpointCounts,
			palette: {
				border: "rgba(148, 163, 184, 0.24)",
				foreground: "rgb(226, 232, 240)",
				muted: "rgb(148, 163, 184)",
			},
			totalMarker:
				'<span style="display:inline-block;width:10px;height:10px;background-color:#5070dd;"></span>',
		});

		expect(tooltipHtml).toContain("Total");
		expect(tooltipHtml).toContain("7 connections");
		expect(tooltipHtml).toContain(endpointALabel);
		expect(tooltipHtml).toContain("4 connections");
		expect(tooltipHtml).toContain(endpointBLabel);
		expect(tooltipHtml).toContain("3 connections");
		expect(tooltipHtml).toContain("width:min(260px,100%);max-width:100%");
		expect(tooltipHtml).not.toContain("min-width:260px");
	});

	it("switches between 24h and 7d windows", () => {
		const onWindowChange = vi.fn();
		render(
			<TcpConnectionUsageView
				window="24h"
				onWindowChange={onWindowChange}
				report={baseReport}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "7d" }));
		expect(onWindowChange).toHaveBeenCalledWith("7d");
		expect(screen.getByText("TCP connection count")).toBeInTheDocument();
		expect(screen.getByText("Connections per minute")).toBeInTheDocument();
	});

	it("aggregates selected endpoints and updates summary cards", () => {
		render(
			<TcpConnectionUsageView
				window="24h"
				onWindowChange={vi.fn()}
				report={baseReport}
			/>,
		);

		expect(
			screen.getByText(/Combined across selected endpoints/i),
		).toBeInTheDocument();
		expect(
			screen.getByRole("img", {
				name: /current 7 connections, peak 7, 2 endpoints selected/i,
			}),
		).toBeInTheDocument();
		expect(screen.getAllByText("current")).toHaveLength(1);
		expect(screen.getAllByText("peak")).toHaveLength(1);
		expect(
			screen.queryByRole("button", { name: "Reset all" }),
		).not.toBeInTheDocument();

		fireEvent.click(screen.getByLabelText(endpointBLabel));

		expect(
			screen.getByRole("img", {
				name: /current 4 connections, peak 4, 1 endpoints selected/i,
			}),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Reset all" }),
		).toBeInTheDocument();
	});

	it("supports isolating one endpoint and restoring all endpoints", () => {
		render(
			<TcpConnectionUsageView
				window="24h"
				onWindowChange={vi.fn()}
				report={baseReport}
			/>,
		);

		fireEvent.click(screen.getAllByRole("button", { name: "Only" })[0]);

		expect(screen.getAllByRole("button", { name: "Only" })[0]).toHaveClass(
			"min-h-11",
		);

		expect(
			screen.getByRole("button", { name: "Reset all" }),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Reset all" })).toHaveClass(
			"min-h-11",
		);
		expect(
			screen.getByRole("img", {
				name: /current 4 connections, peak 4, 1 endpoints selected/i,
			}),
		).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Reset all" }));

		expect(
			screen.queryByRole("button", { name: "Reset all" }),
		).not.toBeInTheDocument();
		expect(
			screen.getByRole("img", {
				name: /current 7 connections, peak 7, 2 endpoints selected/i,
			}),
		).toBeInTheDocument();
	});

	it("shows empty state when no endpoints are selected", () => {
		render(
			<TcpConnectionUsageView
				window="24h"
				onWindowChange={vi.fn()}
				report={baseReport}
			/>,
		);

		fireEvent.click(screen.getByLabelText(endpointALabel));
		fireEvent.click(screen.getByLabelText(endpointBLabel));

		expect(screen.getByText("No endpoints selected")).toBeInTheDocument();
		expect(
			screen.getByText(
				"Select at least one endpoint to inspect the combined TCP connection history.",
			),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Select all endpoints" }),
		).toBeInTheDocument();
	});

	it("shows unsupported state when Linux socket inspection is unavailable", () => {
		render(
			<TcpConnectionUsageView
				window="24h"
				onWindowChange={vi.fn()}
				report={{
					...baseReport,
					warnings: [
						{
							code: "unsupported_platform",
							message:
								"TCP connection count history is currently only supported on Linux nodes.",
						},
					],
				}}
			/>,
		);

		expect(
			screen.getByText("TCP connection count is unavailable"),
		).toBeInTheDocument();
		expect(
			screen.getByText(
				"This node cannot expose Linux socket-level TCP connection counts on the current platform.",
			),
		).toBeInTheDocument();
	});

	it("accepts a fixed tooltip preview minute for stable story rendering", () => {
		render(
			<TcpConnectionUsageView
				window="24h"
				onWindowChange={vi.fn()}
				report={baseReport}
				tooltipPreviewMinute="2026-03-08T00:59:00Z"
			/>,
		);

		expect(screen.getByText("Connections per minute")).toBeInTheDocument();
		expect(
			screen.getByRole("img", {
				name: /current 7 connections, peak 7, 2 endpoints selected/i,
			}),
		).toBeInTheDocument();
	});

	it("shows a nonblocking loading status over the chart", () => {
		render(
			<TcpConnectionUsageView
				window="24h"
				onWindowChange={vi.fn()}
				report={baseReport}
				isWindowPending
			/>,
		);

		expect(
			screen.getByRole("status", { name: "Loading latest data" }),
		).toHaveClass("pointer-events-none");
	});
});
