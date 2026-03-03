import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminAlerts } from "../api/adminAlerts";
import { verifyAdminToken } from "../api/adminAuth";
import { fetchAdminNodesRuntime } from "../api/adminNodeRuntime";
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
vi.mock("../api/adminNodeRuntime");
vi.mock("../api/clusterInfo");
vi.mock("../api/health");
vi.mock("../components/Icon", () => ({
	Icon: ({ ariaLabel }: { ariaLabel?: string }) => (
		<span aria-hidden={ariaLabel ? undefined : "true"} aria-label={ariaLabel} />
	),
}));

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
						status: "up",
						updated_at: "2026-03-01T00:00:00Z",
					},
					components: [
						{
							component: "xp",
							status: "up",
							consecutive_failures: 0,
							recoveries_observed: 1,
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
			],
		});
		vi.mocked(verifyAdminToken).mockResolvedValue(undefined);
	});

	it("adds icon-only links for opening node panels in dashboard nodes table", async () => {
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
		const uniqueHrefs = new Set(
			links.map((link) => link.getAttribute("href")).filter(Boolean),
		);
		expect(uniqueHrefs).toEqual(new Set(["/nodes/node-1", "/nodes/node-2"]));
		expect(links.every((link) => !/\bbtn\b/.test(link.className))).toBe(true);

		for (const nodeName of screen.getAllByText("tokyo-1")) {
			expect(nodeName.closest("a")).toBeNull();
		}
		for (const nodeIdCell of screen.getAllByText("node-1")) {
			expect(nodeIdCell.closest("a")).toBeNull();
		}
	});
});
