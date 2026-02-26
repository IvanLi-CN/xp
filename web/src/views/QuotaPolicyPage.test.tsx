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

function readLegendColors(container: HTMLElement): Record<string, string> {
	const out: Record<string, string> = {};
	const labels = Array.from(container.querySelectorAll("span"));
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

	vi.mocked(putAdminUserNodeWeight).mockResolvedValue({
		node_id: "node-1",
		weight: 0,
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
		const view = renderPage();

		expect(
			await within(view.container).findByText("Weight ratio editor (by node)"),
		).toBeInTheDocument();

		const aliceInput = await within(view.container).findByLabelText(
			"Ratio input for Alice",
		);

		fireEvent.change(aliceInput, { target: { value: "70" } });

		await waitFor(() => {
			expect(within(view.container).getByText("7000")).toBeInTheDocument();
			expect(within(view.container).getByText("3000")).toBeInTheDocument();
		});
	});

	it("keeps local draft on partial save failure and retries only failed rows", async () => {
		vi.mocked(putAdminUserNodeWeight)
			.mockResolvedValueOnce({ node_id: "node-1", weight: 6000 })
			.mockRejectedValueOnce(new Error("boom"))
			.mockResolvedValueOnce({ node_id: "node-1", weight: 4000 });

		const view = renderPage();
		expect(
			await within(view.container).findByText("Weight ratio editor (by node)"),
		).toBeInTheDocument();

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

		const retryButton = within(view.container).getByRole("button", {
			name: "Retry failed rows",
		});
		fireEvent.click(retryButton);

		await waitFor(() => {
			expect(vi.mocked(putAdminUserNodeWeight)).toHaveBeenCalledTimes(3);
		});
		const thirdCall = vi.mocked(putAdminUserNodeWeight).mock.calls[2];
		expect(thirdCall?.[1]).toBe("user-2");
	});

	it("keeps per-user legend color stable after ratio ranking changes", async () => {
		const view = renderPage();

		expect(
			await within(view.container).findByText("Weight ratio editor (by node)"),
		).toBeInTheDocument();
		await within(view.container).findByLabelText("Node weight ratio pie chart");

		const colorsBefore = readLegendColors(view.container);
		expect(colorsBefore.Alice).toBeTruthy();
		expect(colorsBefore.Bob).toBeTruthy();

		const aliceInput = await within(view.container).findByLabelText(
			"Ratio input for Alice",
		);
		fireEvent.change(aliceInput, { target: { value: "30" } });

		await waitFor(() => {
			expect(within(view.container).getByText("3000")).toBeInTheDocument();
			expect(within(view.container).getByText("7000")).toBeInTheDocument();
		});

		const colorsAfter = readLegendColors(view.container);
		expect(colorsAfter.Alice).toBe(colorsBefore.Alice);
		expect(colorsAfter.Bob).toBe(colorsBefore.Bob);
	});
});
