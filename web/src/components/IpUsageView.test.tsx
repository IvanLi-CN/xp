import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AdminNodeIpUsageResponse } from "../api/adminIpUsage";
import { IpUsageView } from "./IpUsageView";

const baseReport: Pick<
	AdminNodeIpUsageResponse,
	| "window_start"
	| "window_end"
	| "warnings"
	| "unique_ip_series"
	| "timeline"
	| "ips"
> = {
	window_start: "2026-03-08T00:00:00Z",
	window_end: "2026-03-08T00:02:00Z",
	warnings: [],
	unique_ip_series: [
		{ minute: "2026-03-08T00:00:00Z", count: 1 },
		{ minute: "2026-03-08T00:01:00Z", count: 2 },
		{ minute: "2026-03-08T00:02:00Z", count: 1 },
	],
	timeline: [
		{
			lane_key: "edge-tokyo|203.0.113.7",
			endpoint_id: "endpoint-1",
			endpoint_tag: "edge-tokyo",
			ip: "203.0.113.7",
			minutes: 2,
			segments: [
				{
					start_minute: "2026-03-08T00:00:00Z",
					end_minute: "2026-03-08T00:01:00Z",
				},
			],
		},
		{
			lane_key: "edge-osaka|198.51.100.4",
			endpoint_id: "endpoint-2",
			endpoint_tag: "edge-osaka",
			ip: "198.51.100.4",
			minutes: 1,
			segments: [
				{
					start_minute: "2026-03-08T00:02:00Z",
					end_minute: "2026-03-08T00:02:00Z",
				},
			],
		},
	],
	ips: [
		{
			ip: "203.0.113.7",
			minutes: 2,
			endpoint_tags: ["edge-tokyo"],
			region: "Japan / Tokyo",
			operator: "ExampleNet",
			last_seen_at: "2026-03-08T00:01:00Z",
		},
		{
			ip: "198.51.100.4",
			minutes: 1,
			endpoint_tags: ["edge-osaka"],
			region: "Singapore",
			operator: "LionLink",
			last_seen_at: "2026-03-08T00:02:00Z",
		},
	],
};

describe("<IpUsageView />", () => {
	it("switches between 24h and 7d windows", () => {
		const onWindowChange = vi.fn();
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				onWindowChange={onWindowChange}
				report={baseReport}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "7d" }));
		expect(onWindowChange).toHaveBeenCalledWith("7d");
		expect(
			screen.getByText("Unique IPs per minute", { selector: "p" }),
		).toBeInTheDocument();
		expect(screen.getByText("IP occupancy lanes")).toBeInTheDocument();
	});

	it("highlights matching rows when hovering and clicking IP/time controls", () => {
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				onWindowChange={vi.fn()}
				report={baseReport}
			/>,
		);

		const ipButton = screen.getAllByRole("button", { name: "203.0.113.7" })[0];
		const tokyoRow = ipButton?.closest("tr");
		const osakaRow = screen
			.getAllByRole("button", { name: "198.51.100.4" })[0]
			?.closest("tr");
		expect(tokyoRow).not.toBeNull();
		expect(osakaRow).not.toBeNull();

		fireEvent.mouseEnter(ipButton);
		expect(screen.getAllByText("203.0.113.7").length).toBeGreaterThan(1);
		expect(tokyoRow).toHaveClass("bg-info/8");
		expect(osakaRow).toHaveClass("opacity-45");

		fireEvent.click(ipButton);
		expect(ipButton).toHaveAttribute("aria-pressed", "true");
		expect(
			screen.getByRole("button", { name: "Clear pinned highlight" }),
		).toBeInTheDocument();

		const lastSeenButton = within(tokyoRow as HTMLTableRowElement).getAllByRole(
			"button",
		)[1];
		fireEvent.click(lastSeenButton);
		expect(lastSeenButton).toHaveAttribute("aria-pressed", "true");
		expect(screen.getByText("Time")).toBeInTheDocument();
	});

	it("shows country.is attribution", () => {
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				geoSource="country_is"
				onWindowChange={vi.fn()}
				report={baseReport}
			/>,
		);

		expect(
			screen.getByText("Geo enrichment uses the free country.is hosted API."),
		).toBeInTheDocument();
	});

	it("shows country.is notice", () => {
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				geoSource="country_is"
				onWindowChange={vi.fn()}
				report={baseReport}
			/>,
		);

		expect(
			screen.getByText("Geo enrichment uses the free country.is hosted API."),
		).toBeInTheDocument();
	});

	it("shows blocking online-stats empty state", () => {
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				onWindowChange={vi.fn()}
				report={{
					...baseReport,
					warnings: [
						{
							code: "online_stats_unavailable",
							message: "statsUserOnline is unavailable.",
						},
					],
					unique_ip_series: [],
					timeline: [],
					ips: [],
				}}
			/>,
		);

		expect(
			screen.getByText("Online snapshots are unavailable"),
		).toBeInTheDocument();
		expect(
			screen.getByText("statsUserOnline is unavailable."),
		).toBeInTheDocument();
	});
});
