import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminAlerts } from "../api/adminAlerts";
import { verifyAdminToken } from "../api/adminAuth";
import { fetchAdminNodes } from "../api/adminNodes";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { HomePage } from "./HomePage";

vi.mock("@tanstack/react-router", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("@tanstack/react-router")>();
	return {
		...actual,
		Link: ({
			children,
			to,
			params,
			...rest
		}: {
			children: React.ReactNode;
			to?: string;
			params?: Record<string, string>;
		}) => {
			let href = to ?? "#";
			if (params) {
				for (const [key, value] of Object.entries(params)) {
					href = href.replace(`$${key}`, value);
				}
			}
			return (
				<a href={href} {...rest}>
					{children}
				</a>
			);
		},
	};
});

vi.mock("../api/adminAlerts");
vi.mock("../api/adminAuth");
vi.mock("../api/adminNodes");
vi.mock("../api/clusterInfo");
vi.mock("../api/health");

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
				<HomePage />
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

describe("<HomePage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		vi.mocked(fetchHealth).mockResolvedValue({ status: "ok" });
		vi.mocked(fetchClusterInfo).mockResolvedValue({
			cluster_id: "cluster-1",
			node_id: "node-1",
			role: "leader",
			leader_api_base_url: "https://node-1.example.com",
			term: 12,
			xp_version: "1.0.0",
		});
		vi.mocked(fetchAdminAlerts).mockResolvedValue({
			partial: false,
			unreachable_nodes: [],
			items: [],
		});
		vi.mocked(fetchAdminNodes).mockResolvedValue({
			items: [
				{
					node_id: "node-1",
					node_name: "tokyo-1",
					api_base_url: "https://node-1.example.com",
					access_host: "node-1.example.com",
					quota_limit_bytes: 0,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
				},
				{
					node_id: "node-2",
					node_name: "osaka-1",
					api_base_url: "https://node-2.example.com",
					access_host: "node-2.example.com",
					quota_limit_bytes: 0,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
				},
			],
		});
		vi.mocked(verifyAdminToken).mockResolvedValue(undefined);
	});

	it("adds icon-only links for opening node panels in dashboard nodes table", async () => {
		renderPage();

		await waitFor(() => {
			expect(fetchAdminNodes).toHaveBeenCalledWith(
				"admintoken",
				expect.any(AbortSignal),
			);
		});

		const links = await screen.findAllByRole("link", {
			name: /open node panel:/i,
		});
		expect(links).toHaveLength(2);
		expect(links[0]).toHaveAttribute("href", "/nodes/node-1");
		expect(links[1]).toHaveAttribute("href", "/nodes/node-2");

		expect(screen.getByText("tokyo-1").closest("a")).toBeNull();
		for (const nodeIdCell of screen.getAllByText("node-1")) {
			expect(nodeIdCell.closest("a")).toBeNull();
		}
	});
});
