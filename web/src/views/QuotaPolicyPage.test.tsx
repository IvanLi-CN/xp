import { QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	waitFor,
	within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminNodes, patchAdminNode } from "../api/adminNodes";
import { fetchAdminQuotaPolicyGlobalWeightRows } from "../api/adminQuotaPolicyGlobalWeightRows";
import {
	fetchAdminQuotaPolicyNodePolicy,
	putAdminQuotaPolicyNodePolicy,
} from "../api/adminQuotaPolicyNodePolicy";
import { fetchAdminQuotaPolicyNodeWeightRows } from "../api/adminQuotaPolicyNodeWeightRows";
import { putAdminUserNodeWeight } from "../api/adminUserNodeWeights";
import { fetchAdminUsers, patchAdminUser } from "../api/adminUsers";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { QuotaPolicyPage } from "./QuotaPolicyPage";

vi.mock("@tanstack/react-router", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("@tanstack/react-router")>();
	return {
		...actual,
		Link: ({
			children,
			to,
			...rest
		}: {
			children: React.ReactNode;
			to?: string;
		}) => (
			<a href={to ?? "#"} {...rest}>
				{children}
			</a>
		),
	};
});

vi.mock("../api/adminNodes");
vi.mock("../api/adminUsers");
vi.mock("../api/adminQuotaPolicyGlobalWeightRows");
vi.mock("../api/adminQuotaPolicyNodePolicy");
vi.mock("../api/adminQuotaPolicyNodeWeightRows");
vi.mock("../api/adminUserNodeWeights");

vi.mock("../components/auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../components/auth")>();
	return {
		...actual,
		readAdminToken: () => "admintoken",
	};
});

function renderPage() {
	const queryClient = createQueryClient();
	return render(
		<QueryClientProvider client={queryClient}>
			<UiPrefsProvider>
				<ToastProvider>
					<QuotaPolicyPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

async function openNodeTab(container: HTMLElement, nodeName = "tokyo-1") {
	const tablist = await within(container).findByRole("tablist", {
		name: "Weight configuration tabs",
	});
	const tab = within(tablist).getByText(nodeName, { selector: "button" });
	fireEvent.click(tab);
}

function readLegendColors(container: HTMLElement): Record<string, string> {
	const out: Record<string, string> = {};
	const legend = within(container).getByTestId("ratio-pie-legend");
	const labels = Array.from(legend.querySelectorAll("span.truncate"));
	for (const labelNode of labels) {
		const label = labelNode.textContent?.trim();
		if (!label) continue;
		const dot = labelNode.previousElementSibling;
		if (!(dot instanceof HTMLElement)) continue;
		if (!dot.style.backgroundColor) continue;
		out[label] = dot.style.backgroundColor;
	}
	return out;
}

function readLegendOrder(container: HTMLElement): string[] {
	const legend = within(container).getByTestId("ratio-pie-legend");
	return Array.from(legend.querySelectorAll("span.truncate"))
		.map((node) => node.textContent?.trim() ?? "")
		.filter(Boolean);
}

function readPieSliceFills(container: HTMLElement): string[] {
	const chart = within(container).getByLabelText("Node weight ratio pie chart");
	return Array.from(chart.querySelectorAll("path"))
		.map((node) => node.getAttribute("fill") ?? "")
		.filter(Boolean);
}

function setupDefaultMocks() {
	vi.mocked(fetchAdminNodes).mockResolvedValue({
		items: [
			{
				node_id: "node-1",
				node_name: "tokyo-1",
				api_base_url: "https://tokyo-1.example.com",
				access_host: "tokyo-1.example.com",
				quota_limit_bytes: 0,
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: null,
				},
			},
		],
	});

	vi.mocked(fetchAdminUsers).mockResolvedValue({
		items: [
			{
				user_id: "user-1",
				display_name: "Alice",
				subscription_token: "sub-1",
				credential_epoch: 0,
				priority_tier: "p1",
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: 480,
				},
			},
			{
				user_id: "user-2",
				display_name: "Bob",
				subscription_token: "sub-2",
				credential_epoch: 0,
				priority_tier: "p2",
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: 480,
				},
			},
		],
	});

	vi.mocked(fetchAdminQuotaPolicyNodeWeightRows).mockResolvedValue({
		items: [
			{
				user_id: "user-1",
				display_name: "Alice",
				priority_tier: "p1",
				endpoint_ids: ["ep-1"],
				stored_weight: 6000,
				editor_weight: 6000,
				source: "explicit",
			},
			{
				user_id: "user-2",
				display_name: "Bob",
				priority_tier: "p2",
				endpoint_ids: ["ep-2"],
				stored_weight: 4000,
				editor_weight: 4000,
				source: "explicit",
			},
		],
	});
	vi.mocked(fetchAdminQuotaPolicyGlobalWeightRows).mockResolvedValue({
		items: [
			{
				user_id: "user-1",
				display_name: "Alice",
				priority_tier: "p1",
				stored_weight: 6000,
				editor_weight: 6000,
				source: "explicit",
			},
			{
				user_id: "user-2",
				display_name: "Bob",
				priority_tier: "p2",
				stored_weight: 4000,
				editor_weight: 4000,
				source: "explicit",
			},
		],
	});
	vi.mocked(fetchAdminQuotaPolicyNodePolicy).mockResolvedValue({
		node_id: "node-1",
		inherit_global: false,
	});

	vi.mocked(putAdminUserNodeWeight).mockResolvedValue({
		node_id: "node-1",
		weight: 0,
	});
	vi.mocked(putAdminQuotaPolicyNodePolicy).mockResolvedValue({
		node_id: "node-1",
		inherit_global: false,
	});
	vi.mocked(patchAdminNode).mockResolvedValue({
		node_id: "node-1",
		node_name: "tokyo-1",
		api_base_url: "https://tokyo-1.example.com",
		access_host: "tokyo-1.example.com",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		},
	});
	vi.mocked(patchAdminUser).mockResolvedValue({
		user_id: "user-1",
		display_name: "Alice",
		subscription_token: "sub-1",
		credential_epoch: 0,
		priority_tier: "p1",
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: 480,
		},
	});
}

describe("<QuotaPolicyPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		setupDefaultMocks();
	});

	afterEach(() => {
		cleanup();
	});

	it("keeps slider/input linkage and recomputes weights", async () => {
		const originalInnerWidth = window.innerWidth;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: 700,
		});
		try {
			const view = renderPage();
			await openNodeTab(view.container);

			const aliceInput = await within(view.container).findByLabelText(
				"Ratio input for Alice",
			);

			fireEvent.change(aliceInput, { target: { value: "70" } });

			await waitFor(() => {
				expect(within(view.container).getByText("7000")).toBeInTheDocument();
				expect(within(view.container).getByText("3000")).toBeInTheDocument();
			});
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});

	it("keeps local draft on partial save failure and retries only failed rows", async () => {
		vi.mocked(putAdminUserNodeWeight)
			.mockResolvedValueOnce({ node_id: "node-1", weight: 6000 })
			.mockRejectedValueOnce(new Error("boom"))
			.mockResolvedValueOnce({ node_id: "node-1", weight: 4000 });

		const originalInnerWidth = window.innerWidth;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: 700,
		});
		try {
			const view = renderPage();
			await openNodeTab(view.container);

			const aliceInput = await within(view.container).findByLabelText(
				"Ratio input for Alice",
			);
			fireEvent.change(aliceInput, { target: { value: "65" } });

			const saveButton = within(view.container).getByRole("button", {
				name: "Save ratios",
			});
			fireEvent.click(saveButton);

			await waitFor(() => {
				expect(
					within(view.container).getByText(/Failed rows \(1\)/),
				).toBeInTheDocument();
			});
			expect(vi.mocked(putAdminUserNodeWeight)).toHaveBeenCalledTimes(2);

			const retryButtons = within(view.container).getAllByRole("button", {
				name: "Retry failed rows",
			});
			const retryButton =
				retryButtons.find((button) => !button.hasAttribute("disabled")) ??
				retryButtons[0];
			if (!retryButton) {
				throw new Error("expected retry button");
			}
			fireEvent.click(retryButton);

			await waitFor(() => {
				expect(vi.mocked(putAdminUserNodeWeight)).toHaveBeenCalledTimes(3);
			});
			const thirdCall = vi.mocked(putAdminUserNodeWeight).mock.calls[2];
			expect(thirdCall?.[1]).toBe("user-2");
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});

	it("disables node editor in inherit mode and enables after turning inherit off", async () => {
		vi.mocked(fetchAdminQuotaPolicyNodePolicy)
			.mockResolvedValueOnce({
				node_id: "node-1",
				inherit_global: true,
			})
			.mockResolvedValue({
				node_id: "node-1",
				inherit_global: false,
			});
		vi.mocked(putAdminQuotaPolicyNodePolicy).mockResolvedValue({
			node_id: "node-1",
			inherit_global: false,
		});

		const originalInnerWidth = window.innerWidth;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: 700,
		});
		try {
			const view = renderPage();
			await openNodeTab(view.container);

			const aliceInput = await within(view.container).findByLabelText(
				"Ratio input for Alice",
			);
			expect(aliceInput).toBeDisabled();

			const inheritToggle = within(view.container).getByRole("checkbox", {
				name: "Inherit global default ratios",
			});
			fireEvent.click(inheritToggle);

			await waitFor(() => {
				expect(vi.mocked(putAdminQuotaPolicyNodePolicy)).toHaveBeenCalledWith(
					"admintoken",
					"node-1",
					false,
				);
			});

			await waitFor(() => {
				expect(
					within(view.container).getByLabelText("Ratio input for Alice"),
				).not.toBeDisabled();
			});
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});

	it("keeps pie legend and slice order stable after ratio ranking changes", async () => {
		const originalInnerWidth = window.innerWidth;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: 700,
		});
		try {
			const view = renderPage();
			await openNodeTab(view.container);
			await within(view.container).findByLabelText(
				"Node weight ratio pie chart",
			);

			const colorsBefore = readLegendColors(view.container);
			const orderBefore = readLegendOrder(view.container);
			const firstSliceBefore = readPieSliceFills(view.container)[0];
			expect(colorsBefore.Alice).toBeTruthy();
			expect(colorsBefore.Bob).toBeTruthy();
			expect(orderBefore).toEqual(["Alice", "Bob"]);
			expect(firstSliceBefore).toBeTruthy();

			const aliceInput = await within(view.container).findByLabelText(
				"Ratio input for Alice",
			);
			fireEvent.change(aliceInput, { target: { value: "30" } });

			await waitFor(() => {
				expect(within(view.container).getByText("3000")).toBeInTheDocument();
				expect(within(view.container).getByText("7000")).toBeInTheDocument();
			});

			const colorsAfter = readLegendColors(view.container);
			const orderAfter = readLegendOrder(view.container);
			const firstSliceAfter = readPieSliceFills(view.container)[0];
			expect(colorsAfter.Alice).toBe(colorsBefore.Alice);
			expect(colorsAfter.Bob).toBe(colorsBefore.Bob);
			expect(orderAfter).toEqual(orderBefore);
			expect(firstSliceAfter).toBe(firstSliceBefore);
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});

	it("uses list layout below md for both global and node editors", async () => {
		const originalInnerWidth = window.innerWidth;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: 700,
		});
		try {
			const view = renderPage();
			await within(view.container).findByLabelText(
				"Global ratio input for Alice",
			);
			const globalPanel = within(view.container).getByTestId(
				"global-ratio-editor-panel",
			);
			expect(globalPanel).toHaveAttribute("data-layout", "list");
			expect(globalPanel).toHaveAttribute("data-width-tier", "sm");
			expect(globalPanel).toHaveAttribute("data-panel-tier", "sm");
			expect(globalPanel).toHaveClass(
				"layout-list",
				"width-tier-sm",
				"panel-tier-sm",
			);
			expect(
				within(view.container).getByTestId("global-ratio-editor-list"),
			).toBeInTheDocument();
			expect(
				within(view.container).queryByTestId("global-ratio-editor-table"),
			).toBeNull();

			await openNodeTab(view.container);
			await within(view.container).findByLabelText("Ratio input for Alice");
			const nodePanel = within(view.container).getByTestId(
				"ratio-editor-panel",
			);
			expect(nodePanel).toHaveAttribute("data-layout", "list");
			expect(nodePanel).toHaveAttribute("data-width-tier", "sm");
			expect(nodePanel).toHaveAttribute("data-panel-tier", "sm");
			expect(nodePanel).toHaveClass(
				"layout-list",
				"width-tier-sm",
				"panel-tier-sm",
			);
			expect(
				within(view.container).getByTestId("ratio-editor-list"),
			).toBeInTheDocument();
			expect(
				within(view.container).queryByTestId("ratio-editor-table"),
			).toBeNull();
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});

	it("uses table layout from md and above for both global and node editors", async () => {
		const originalInnerWidth = window.innerWidth;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: 768,
		});
		try {
			const view = renderPage();
			await within(view.container).findByTestId("global-ratio-editor-table");
			const globalPanel = within(view.container).getByTestId(
				"global-ratio-editor-panel",
			);
			expect(globalPanel).toHaveAttribute("data-layout", "table");
			expect(globalPanel).toHaveAttribute("data-width-tier", "md");
			expect(globalPanel).toHaveAttribute("data-panel-tier", "md");
			expect(globalPanel).toHaveClass(
				"layout-table",
				"width-tier-md",
				"panel-tier-md",
			);
			expect(
				within(view.container).getByTestId("global-ratio-editor-table"),
			).toBeInTheDocument();
			expect(
				within(view.container).getByTestId("global-ratio-editor-table")
					.className,
			).not.toMatch(/min-w-\[/);
			expect(
				within(view.container).queryByTestId("global-ratio-editor-list"),
			).toBeNull();

			await openNodeTab(view.container);
			await within(view.container).findByTestId("ratio-editor-table");
			const nodePanel = within(view.container).getByTestId(
				"ratio-editor-panel",
			);
			expect(nodePanel).toHaveAttribute("data-layout", "table");
			expect(nodePanel).toHaveAttribute("data-width-tier", "md");
			expect(nodePanel).toHaveAttribute("data-panel-tier", "md");
			expect(nodePanel).toHaveClass(
				"layout-table",
				"width-tier-md",
				"panel-tier-md",
			);
			expect(
				within(view.container).getByTestId("ratio-editor-table"),
			).toBeInTheDocument();
			expect(
				within(view.container).getByTestId("ratio-editor-table").className,
			).not.toMatch(/min-w-\[/);
			expect(
				within(view.container).queryByTestId("ratio-editor-list"),
			).toBeNull();
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});

	it("hides user id and removes standalone input column in table layout", async () => {
		const originalInnerWidth = window.innerWidth;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: 1024,
		});
		try {
			const view = renderPage();
			const globalTable = await within(view.container).findByTestId(
				"global-ratio-editor-table",
			);
			expect(within(globalTable).queryByText("Input (%)")).toBeNull();
			expect(within(globalTable).queryByText("user-1")).toBeNull();
			expect(within(globalTable).queryByText("user-2")).toBeNull();

			await openNodeTab(view.container);
			const nodeTable = await within(view.container).findByTestId(
				"ratio-editor-table",
			);
			expect(within(nodeTable).queryByText("Input (%)")).toBeNull();
			expect(within(nodeTable).queryByText("user-1")).toBeNull();
			expect(within(nodeTable).queryByText("user-2")).toBeNull();
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});

	it("supports inline percent edit in table on double click and enter", async () => {
		const originalInnerWidth = window.innerWidth;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: 1024,
		});
		try {
			const view = renderPage();
			await within(view.container).findByTestId("global-ratio-editor-table");

			const globalDisplay = within(view.container).getByTestId(
				"global-ratio-table-percent-user-1-display",
			);
			fireEvent.doubleClick(globalDisplay);
			const globalInput = within(view.container).getByTestId(
				"global-ratio-table-percent-user-1-input",
			);
			fireEvent.change(globalInput, { target: { value: "60" } });
			fireEvent.keyDown(globalInput, { key: "Enter" });

			await waitFor(() => {
				expect(
					within(view.container).queryByTestId(
						"global-ratio-table-percent-user-1-input",
					),
				).toBeNull();
			});
			expect(
				within(view.container).getByTestId(
					"global-ratio-table-percent-user-1-display",
				),
			).toHaveTextContent("60.00%");

			await openNodeTab(view.container);
			await within(view.container).findByTestId("ratio-editor-table");
			const nodeDisplay = within(view.container).getByTestId(
				"ratio-table-percent-user-1-display",
			);
			fireEvent.doubleClick(nodeDisplay);
			const nodeInput = within(view.container).getByTestId(
				"ratio-table-percent-user-1-input",
			);
			fireEvent.change(nodeInput, { target: { value: "70" } });
			fireEvent.keyDown(nodeInput, { key: "Enter" });

			await waitFor(() => {
				expect(
					within(view.container).queryByTestId(
						"ratio-table-percent-user-1-input",
					),
				).toBeNull();
			});
			expect(
				within(view.container).getByTestId(
					"ratio-table-percent-user-1-display",
				),
			).toHaveTextContent("70.00%");
			expect(within(view.container).getByText("7000")).toBeInTheDocument();
			expect(within(view.container).getByText("3000")).toBeInTheDocument();
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});

	it("covers all viewport breakpoints with width-tier markers", async () => {
		const originalInnerWidth = window.innerWidth;
		const cases = [
			{ width: 360, widthTier: "xs", layout: "list" },
			{ width: 640, widthTier: "sm", layout: "list" },
			{ width: 768, widthTier: "md", layout: "table" },
			{ width: 1024, widthTier: "lg", layout: "table" },
			{ width: 1280, widthTier: "xl", layout: "table" },
			{ width: 1536, widthTier: "2xl", layout: "table" },
		] as const;

		try {
			for (const testCase of cases) {
				Object.defineProperty(window, "innerWidth", {
					configurable: true,
					writable: true,
					value: testCase.width,
				});

				const view = renderPage();
				if (testCase.layout === "list") {
					await within(view.container).findByLabelText(
						"Global ratio input for Alice",
					);
				} else {
					await within(view.container).findByTestId(
						"global-ratio-editor-table",
					);
				}
				const globalPanel = within(view.container).getByTestId(
					"global-ratio-editor-panel",
				);
				expect(globalPanel).toHaveAttribute("data-layout", testCase.layout);
				expect(globalPanel).toHaveAttribute(
					"data-width-tier",
					testCase.widthTier,
				);
				expect(globalPanel).toHaveAttribute(
					"data-panel-tier",
					testCase.widthTier,
				);

				await openNodeTab(view.container);
				if (testCase.layout === "list") {
					await within(view.container).findByLabelText("Ratio input for Alice");
				} else {
					await within(view.container).findByTestId("ratio-editor-table");
				}
				const nodePanel = within(view.container).getByTestId(
					"ratio-editor-panel",
				);
				expect(nodePanel).toHaveAttribute("data-layout", testCase.layout);
				expect(nodePanel).toHaveAttribute(
					"data-width-tier",
					testCase.widthTier,
				);
				expect(nodePanel).toHaveAttribute(
					"data-panel-tier",
					testCase.widthTier,
				);

				view.unmount();
			}
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
		}
	});

	it("keeps table layout on desktop when the editor panel narrows", async () => {
		const originalInnerWidth = window.innerWidth;
		const originalResizeObserver = window.ResizeObserver;
		Object.defineProperty(window, "innerWidth", {
			configurable: true,
			writable: true,
			value: 1366,
		});
		Object.defineProperty(window, "ResizeObserver", {
			configurable: true,
			writable: true,
			value: undefined,
		});
		try {
			const view = renderPage();
			await openNodeTab(view.container);

			await within(view.container).findByTestId("ratio-editor-table");
			const panel = within(view.container).getByTestId("ratio-editor-panel");
			Object.defineProperty(panel, "clientWidth", {
				configurable: true,
				get: () => 700,
			});

			window.dispatchEvent(new Event("resize"));

			await waitFor(() => {
				expect(
					within(view.container).getByTestId("ratio-editor-table"),
				).toBeInTheDocument();
			});
			expect(panel).toHaveAttribute("data-layout", "table");
			expect(panel).toHaveAttribute("data-width-tier", "xl");
			expect(panel).toHaveAttribute("data-panel-tier", "sm");
			expect(panel).toHaveClass(
				"layout-table",
				"width-tier-xl",
				"panel-tier-sm",
			);
			expect(
				within(view.container).queryByTestId("ratio-editor-list"),
			).toBeNull();
		} finally {
			Object.defineProperty(window, "innerWidth", {
				configurable: true,
				writable: true,
				value: originalInnerWidth,
			});
			Object.defineProperty(window, "ResizeObserver", {
				configurable: true,
				writable: true,
				value: originalResizeObserver,
			});
		}
	});
});
