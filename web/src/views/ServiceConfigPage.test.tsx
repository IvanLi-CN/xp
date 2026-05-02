import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminConfig } from "../api/adminConfig";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { ToastProvider } from "../components/Toast";
import { createQueryClient } from "../queryClient";
import { ServiceConfigPage } from "./ServiceConfigPage";

vi.mock("../api/adminConfig");
vi.mock("../api/clusterInfo");
vi.mock("../api/health");

const { mockReadAdminToken } = vi.hoisted(() => ({
	mockReadAdminToken: vi.fn(() => "admintoken"),
}));

vi.mock("../components/auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../components/auth")>();
	return {
		...actual,
		readAdminToken: mockReadAdminToken,
	};
});

function renderPage() {
	const queryClient = createQueryClient();
	return render(
		<QueryClientProvider client={queryClient}>
			<ToastProvider>
				<ServiceConfigPage />
			</ToastProvider>
		</QueryClientProvider>,
	);
}

function setupMocks() {
	vi.mocked(fetchHealth).mockResolvedValue({ status: "ok" });
	vi.mocked(fetchClusterInfo).mockResolvedValue({
		cluster_id: "cluster-alpha",
		node_id: "node-1",
		role: "leader",
		leader_api_base_url: "https://node-1.example.com",
		term: 7,
		xp_version: "0.2.0",
	});
	vi.mocked(fetchAdminConfig).mockResolvedValue({
		bind: "127.0.0.1:62416",
		xray_api_addr: "127.0.0.1:10085",
		data_dir: "./data",
		node_name: "node-1",
		access_host: "edge.example.com",
		api_base_url: "https://api.example.com",
		quota_poll_interval_secs: 10,
		quota_auto_unban: true,
		ip_geo_enabled: false,
		ip_geo_origin: "https://api.country.is",
		admin_token_present: true,
		admin_token_masked: "********",
	});
}

describe("ServiceConfigPage", () => {
	beforeEach(() => {
		setupMocks();
	});

	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
	});

	it("shows provider-only mihomo delivery in the summary", async () => {
		renderPage();
		expect(await screen.findByText("mihomo")).toBeTruthy();
		expect(
			await screen.findByText(/Mihomo uses provider-only delivery/),
		).toBeTruthy();
		expect(screen.queryByText("Mihomo delivery")).toBeNull();
	});
});
