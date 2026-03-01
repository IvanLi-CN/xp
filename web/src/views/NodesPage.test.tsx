import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createAdminJoinToken } from "../api/adminJoinTokens";
import { fetchAdminNodesRuntime } from "../api/adminNodeRuntime";
import { fetchClusterInfo } from "../api/clusterInfo";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { NodesPage } from "./NodesPage";

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

vi.mock("../api/adminJoinTokens");
vi.mock("../api/adminNodeRuntime");
vi.mock("../api/clusterInfo");

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
					<NodesPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

describe("<NodesPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		globalThis.ResizeObserver = class {
			observe() {
				// no-op for jsdom layout tests
			}
			disconnect() {
				// no-op for jsdom layout tests
			}
			unobserve() {
				// no-op for jsdom layout tests
			}
		} as typeof ResizeObserver;

		vi.mocked(fetchClusterInfo).mockResolvedValue({
			cluster_id: "cluster-1",
			node_id: "node-1",
			role: "leader",
			leader_api_base_url: "https://node-1.example.com",
			term: 12,
			xp_version: "1.0.0",
		});
		vi.mocked(createAdminJoinToken).mockResolvedValue({
			join_token: "join-token-1",
		});
		vi.mocked(fetchAdminNodesRuntime).mockResolvedValue({
			partial: false,
			unreachable_nodes: [],
			items: [
				{
					node_id: "node-1",
					node_name: "tokyo-1",
					api_base_url: "https://node-1.example.com",
					access_host: "node-1.example.com",
					summary: {
						status: "up",
						updated_at: "2026-03-01T00:00:00Z",
					},
					components: [
						{
							component: "xp",
							status: "up",
							consecutive_failures: 0,
							recoveries_observed: 2,
							restart_attempts: 0,
						},
					],
					recent_slots: [
						{
							slot_start: "2026-03-01T00:00:00Z",
							status: "up",
						},
					],
				},
				{
					node_id: "node-2",
					node_name: "osaka-1",
					api_base_url: "https://node-2.example.com",
					access_host: "node-2.example.com",
					summary: {
						status: "degraded",
						updated_at: "2026-03-01T00:00:00Z",
					},
					components: [
						{
							component: "xray",
							status: "down",
							consecutive_failures: 3,
							recoveries_observed: 1,
							restart_attempts: 1,
						},
					],
					recent_slots: [
						{
							slot_start: "2026-03-01T00:00:00Z",
							status: "degraded",
						},
					],
				},
			],
		});
	});

	it("uses icon-only links to open node panel in node inventory rows", async () => {
		renderPage();

		await waitFor(() => {
			expect(fetchAdminNodesRuntime).toHaveBeenCalledWith(
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
		expect(screen.getByText("node-1").closest("a")).toBeNull();
	});
});
