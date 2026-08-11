import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

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
			cluster_id: fixtureCatalog.cluster.fixture84(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			role: "leader",
			leader_api_base_url: fixtureCatalog.service.fixture178(),
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
					node_id: fixtureCatalog.nodeId.fixture32(),
					node_name: fixtureCatalog.nodeName.fixture33(),
					api_base_url: fixtureCatalog.service.fixture178(),
					access_host: fixtureCatalog.host.fixture179(),
					summary: {
						status: "up",
						updated_at: fixtureCatalog.timestamp.t20260301T000000(),
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
							slot_start: fixtureCatalog.timestamp.t20260301T000000(),
							status: "up",
						},
					],
				},
				{
					node_id: fixtureCatalog.nodeId.fixture36(),
					node_name: fixtureCatalog.nodeName.fixture37(),
					api_base_url: fixtureCatalog.service.fixture180(),
					access_host: fixtureCatalog.host.fixture181(),
					summary: {
						status: "up",
						updated_at: fixtureCatalog.timestamp.t20260301T000000(),
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
							slot_start: fixtureCatalog.timestamp.t20260301T000000(),
							status: "up",
						},
					],
				},
			],
		});
		vi.mocked(verifyAdminToken).mockResolvedValue(undefined);
	});

	it("reuses the shared inventory actions on the dashboard", async () => {
		renderPage();

		await waitFor(() => {
			expect(fetchAdminNodesRuntime).toHaveBeenCalledWith(
				"admintoken",
				expect.any(AbortSignal),
			);
		});

		const detailsLinks = await screen.findAllByRole("link", {
			name: "Details",
		});
		expect(detailsLinks.map((link) => link.getAttribute("href"))).toEqual([
			`/nodes/${fixtureCatalog.nodeId.fixture32()}`,
			`/nodes/${fixtureCatalog.nodeId.fixture36()}`,
		]);
		expect(screen.getAllByRole("link", { name: "Open on node" })).toHaveLength(
			2,
		);
	});
});
