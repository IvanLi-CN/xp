import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

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
		cluster_id: fixtureCatalog.cluster.fixture53(),
		node_id: fixtureCatalog.nodeId.fixture32(),
		role: "leader",
		leader_api_base_url: fixtureCatalog.service.fixture178(),
		term: 7,
		xp_version: "0.2.0",
	});
	vi.mocked(fetchAdminConfig).mockResolvedValue({
		bind: fixtureCatalog.address.loopbackPort39058(),
		xray_api_addr: fixtureCatalog.address.loopbackPort39059(),
		data_dir: "./data",
		node_name: fixtureCatalog.nodeId.fixture32(),
		access_host: fixtureCatalog.host.fixture266(),
		api_base_url: fixtureCatalog.service.fixture267(),
		vless_https_canary_bind: "127.0.0.1:39043",
		quota_poll_interval_secs: 10,
		quota_auto_unban: true,
		ip_geo_enabled: false,
		ip_geo_origin: "https://api.country.is",
		mihomo_resource_allow_private_targets: false,
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

	it("directs private-target configuration to node details", async () => {
		renderPage();
		expect(
			await screen.findByRole("heading", { name: "Cluster settings" }),
		).toBeTruthy();
		expect(
			await screen.findByRole("heading", { name: "Node settings" }),
		).toBeTruthy();

		expect(
			await screen.findByText(/configured per node from the node details page/),
		).toBeTruthy();
		expect(
			screen.queryByText("Allow private Mihomo mirror targets"),
		).toBeNull();
	});
});
