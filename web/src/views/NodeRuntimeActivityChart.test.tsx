import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { NodeRuntimeHistorySlot } from "../api/adminNodeRuntime";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { NodeRuntimeActivityChart } from "./NodeRuntimeActivityChart";

const recentSlots: NodeRuntimeHistorySlot[] = [
	{ slot_start: fixtureCatalog.timestamp.t20260308T000000(), status: "up" },
	{
		slot_start: fixtureCatalog.timestamp.t20260308T003000(),
		status: "degraded",
	},
];

afterEach(() => {
	document.body.replaceChildren();
});

describe("<NodeRuntimeActivityChart />", () => {
	it("keeps the runtime activity chart in an operable local scroll region", async () => {
		render(<NodeRuntimeActivityChart recentSlots={recentSlots} />);

		const chart = screen.getByRole("region", {
			name: "7-day service activity chart",
		});
		expect(chart).toHaveAttribute("aria-label", "7-day service activity chart");
		expect(chart).toHaveClass(
			"max-w-full",
			"overflow-x-auto",
			"overscroll-x-contain",
		);
		expect(screen.getByText("00:00")).toBeTruthy();
		expect(screen.getByText("24:00")).toBeTruthy();
		expect(chart.firstElementChild).toHaveClass("min-w-[28rem]");
		expect(chart.querySelector("span.font-mono")).toHaveClass(
			"sticky",
			"left-0",
		);

		Object.defineProperties(chart, {
			clientWidth: { configurable: true, value: 320 },
			scrollWidth: { configurable: true, value: 448 },
		});
		fireEvent.scroll(chart);

		const rightButton = screen.getByRole("button", {
			name: "Scroll activity chart right",
		});
		await waitFor(() => expect(rightButton).toBeEnabled());
		fireEvent.click(rightButton);
		expect(chart.scrollLeft).toBeGreaterThan(0);
		await waitFor(() =>
			expect(
				screen.getByRole("button", {
					name: "Scroll activity chart left",
				}),
			).toBeEnabled(),
		);
	});
});
