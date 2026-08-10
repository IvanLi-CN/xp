import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

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
	window_start_at: fixtureCatalog.timestamp.baseline(),
	window_end_at: fixtureCatalog.timestamp.recent(),
	timezone: "UTC",
	summary: {
		mode: "cycle",
		uplink_bytes: fixtureCatalog.number.value1(),
		downlink_bytes: fixtureCatalog.number.value1(),
		total_bytes: fixtureCatalog.number.value1(),
		complete: true,
	},
	current: [
		{
			start_at: fixtureCatalog.timestamp.baseline(),
			end_at: fixtureCatalog.timestamp.t20260729T185500(),
			uplink_bytes: fixtureCatalog.number.value3(),
			downlink_bytes: fixtureCatalog.number.value4(),
			total_bytes: fixtureCatalog.number.value7(),
			complete: true,
			is_current_day: false,
		},
	],
	reference: [
		{
			start_at: fixtureCatalog.timestamp.earlier(),
			end_at: fixtureCatalog.timestamp.baseline(),
			uplink_bytes: fixtureCatalog.number.value3(),
			downlink_bytes: null,
			total_bytes: null,
			complete: false,
			is_current_day: false,
		},
	],
	partial: false,
	last_sample_at: fixtureCatalog.timestamp.t20260729T185500(),
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

		expect(tooltip).toContain("2024-01-01 00:00:00 UTC");
		expect(tooltip).toContain("Current");
		expect(tooltip).toContain("Previous 24h");
		expect(tooltip).toContain("up 0 MiB");
		expect(tooltip).toContain("down 0 MiB");
		expect(tooltip).toContain("total 0 MiB");
		expect(tooltip).toContain("down -");
		expect(tooltip).toContain("color:var(--popover-foreground)");
		expect(tooltip).toContain("color:var(--muted-foreground)");
		expect(tooltip).toContain("width:min(240px,100%);max-width:100%");
		expect(tooltip).not.toContain("min-width");
	});
});
