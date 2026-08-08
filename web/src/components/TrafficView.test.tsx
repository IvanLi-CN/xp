import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TrafficReport } from "../api/adminTraffic";
import { TrafficView, buildTrafficTooltipHtml } from "./TrafficView";

const tooltipPalette = {
	background: "var(--popover)",
	border: "var(--border)",
	foreground: "var(--popover-foreground)",
	muted: "var(--muted-foreground)",
	shadow: "var(--xp-overlay)",
	shadowBlur: 18,
	shadowOffsetX: 0,
	shadowOffsetY: 10,
};

const report: TrafficReport = {
	window: "24h",
	window_start_at: "2026-07-29T00:00:00Z",
	window_end_at: "2026-07-30T00:00:00Z",
	timezone: "UTC",
	summary: {
		mode: "cycle",
		uplink_bytes: 0,
		downlink_bytes: 0,
		total_bytes: 0,
		complete: true,
	},
	current: [
		{
			start_at: "2026-07-29T18:50:00Z",
			end_at: "2026-07-29T18:55:00Z",
			uplink_bytes: 10_485,
			downlink_bytes: 31_457,
			total_bytes: 41_942,
			complete: true,
			is_current_day: false,
		},
	],
	reference: [
		{
			start_at: "2026-07-28T18:50:00Z",
			end_at: "2026-07-28T18:55:00Z",
			uplink_bytes: 5_242,
			downlink_bytes: null,
			total_bytes: null,
			complete: false,
			is_current_day: false,
		},
	],
	partial: false,
	last_sample_at: "2026-07-29T18:55:00Z",
	warnings: [],
};

describe("buildTrafficTooltipHtml", () => {
	it("shows a nonblocking loading status over the chart", () => {
		render(
			<TrafficView
				report={report}
				window="24h"
				onWindowChange={vi.fn()}
				isWindowPending
			/>,
		);

		expect(
			screen.getByRole("status", { name: "Loading latest data" }),
		).toHaveClass("pointer-events-none");
	});

	it("renders current and reference byte totals with themed foregrounds", () => {
		const tooltip = buildTrafficTooltipHtml(report, "24h", tooltipPalette, 0);

		expect(tooltip).toContain("2026-07-29 18:50:00 UTC");
		expect(tooltip).toContain("Current");
		expect(tooltip).toContain("Previous 24h");
		expect(tooltip).toContain("up 0.01 MiB");
		expect(tooltip).toContain("down 0.03 MiB");
		expect(tooltip).toContain("total 0.04 MiB");
		expect(tooltip).toContain("down -");
		expect(tooltip).toContain("color:var(--popover-foreground)");
		expect(tooltip).toContain("color:var(--muted-foreground)");
		expect(tooltip).toContain("width:min(240px,100%);max-width:100%");
		expect(tooltip).not.toContain("min-width");
	});

	it("escapes an invalid timestamp before returning tooltip HTML", () => {
		const invalidTimestampReport = {
			...report,
			current: [{ ...report.current[0], start_at: "<invalid>" }],
		};
		const tooltip = buildTrafficTooltipHtml(
			invalidTimestampReport,
			"24h",
			tooltipPalette,
			0,
		);

		expect(tooltip).toContain("&lt;invalid&gt;");
		expect(tooltip).not.toContain("<invalid>");
	});
});
