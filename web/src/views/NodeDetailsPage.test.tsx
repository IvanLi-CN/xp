import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
	fetchAdminNode,
	fetchAdminNodeQuotaStatus,
	patchAdminNode,
	patchAdminNodeQuotaUsage,
} from "../api/adminNodes";
import type { NodeQuotaReset } from "../api/quotaReset";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { NodeDetailsPage } from "./NodeDetailsPage";

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
		useNavigate: () => vi.fn(),
		useParams: () => ({ nodeId: "n_01HNODEAAAAAA" }),
	};
});

vi.mock("../api/adminNodes");

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
					<NodeDetailsPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

function setupNodeMock(args: {
	quota_reset: NodeQuotaReset;
	quota_limit_bytes: number;
}) {
	vi.mocked(fetchAdminNode).mockResolvedValue({
		node_id: "n_01HNODEAAAAAA",
		node_name: "node-a",
		api_base_url: "https://node-a.example.com",
		access_host: "node-a.example.com",
		quota_reset: args.quota_reset,
		quota_limit_bytes: args.quota_limit_bytes,
	});
	vi.mocked(fetchAdminNodeQuotaStatus).mockResolvedValue({
		partial: false,
		unreachable_nodes: [],
		items: [
			{
				node_id: "n_01HNODEAAAAAA",
				quota_limit_bytes: args.quota_limit_bytes,
				used_bytes: 0,
				remaining_bytes: args.quota_limit_bytes,
				cycle_end_at: null,
				exhausted: false,
				exhausted_at: null,
				warning: null,
			},
		],
	});
}

describe("<NodeDetailsPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
	});

	afterEach(() => {
		// RTL's cleanup is automatic in newer versions, but keep explicit behavior stable.
		document.body.innerHTML = "";
	});

	it("requires tz_offset_minutes when setting monthly node limit", async () => {
		setupNodeMock({
			quota_reset: {
				policy: "monthly",
				day_of_month: 1,
				tz_offset_minutes: null,
			},
			quota_limit_bytes: 0,
		});

		const view = renderPage();

		// Wait for node metadata to appear.
		expect(
			await within(view.container).findByRole("heading", {
				name: "Node metadata",
			}),
		).toBeInTheDocument();

		fireEvent.change(within(view.container).getByLabelText("Mode"), {
			target: { value: "monthly_limit" },
		});
		fireEvent.change(within(view.container).getByPlaceholderText("e.g. 2TiB"), {
			target: { value: "1TiB" },
		});

		fireEvent.click(
			within(view.container).getByRole("button", { name: "Save changes" }),
		);

		expect(
			await within(view.container).findByText(
				"tz_offset_minutes is required for monthly node limit.",
			),
		).toBeInTheDocument();
		expect(vi.mocked(patchAdminNode)).not.toHaveBeenCalled();
	});

	it("sends quota_limit_bytes and monthly quota_reset when saving monthly node limit", async () => {
		setupNodeMock({
			quota_reset: {
				policy: "monthly",
				day_of_month: 1,
				tz_offset_minutes: null,
			},
			quota_limit_bytes: 0,
		});
		vi.mocked(patchAdminNode).mockResolvedValue({
			node_id: "n_01HNODEAAAAAA",
			node_name: "node-a",
			api_base_url: "https://node-a.example.com",
			access_host: "node-a.example.com",
			quota_reset: {
				policy: "monthly",
				day_of_month: 1,
				tz_offset_minutes: 480,
			},
			quota_limit_bytes: 1 * 2 ** 40,
		});

		const view = renderPage();
		await within(view.container).findByRole("heading", {
			name: "Node metadata",
		});

		fireEvent.change(within(view.container).getByLabelText("Mode"), {
			target: { value: "monthly_limit" },
		});
		fireEvent.change(within(view.container).getByPlaceholderText("e.g. 2TiB"), {
			target: { value: "1TiB" },
		});
		fireEvent.change(
			within(view.container).getByLabelText(/tz_offset_minutes/),
			{
				target: { value: "480" },
			},
		);

		fireEvent.click(
			within(view.container).getByRole("button", { name: "Save changes" }),
		);

		await waitFor(() => {
			expect(vi.mocked(patchAdminNode)).toHaveBeenCalledWith(
				"admintoken",
				"n_01HNODEAAAAAA",
				expect.objectContaining({
					quota_limit_bytes: 1 * 2 ** 40,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: 480,
					},
				}),
			);
		});
	});

	it("calls quota-usage override API with used_bytes and sync_baseline", async () => {
		setupNodeMock({
			quota_reset: {
				policy: "monthly",
				day_of_month: 1,
				tz_offset_minutes: 480,
			},
			quota_limit_bytes: 1 * 2 ** 40,
		});
		vi.mocked(patchAdminNodeQuotaUsage).mockResolvedValue({
			status: {
				node_id: "n_01HNODEAAAAAA",
				quota_limit_bytes: 1 * 2 ** 40,
				used_bytes: 123 * 2 ** 30,
				remaining_bytes: 1 * 2 ** 40 - 123 * 2 ** 30,
				cycle_end_at: null,
				exhausted: false,
				exhausted_at: null,
				warning: null,
			},
			synced_baseline: true,
			warning: null,
		});

		const view = renderPage();
		await within(view.container).findByRole("heading", {
			name: "Node metadata",
		});

		fireEvent.change(within(view.container).getByLabelText("Set used"), {
			target: { value: "123GiB" },
		});
		fireEvent.click(
			within(view.container).getByRole("button", { name: "Apply override" }),
		);

		await waitFor(() => {
			expect(vi.mocked(patchAdminNodeQuotaUsage)).toHaveBeenCalledWith(
				"admintoken",
				"n_01HNODEAAAAAA",
				{ used_bytes: 123 * 2 ** 30, sync_baseline: true },
			);
		});
	});
});
