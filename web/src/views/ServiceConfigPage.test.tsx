import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { fetchAdminConfig } from "../api/adminConfig";
import { fetchAdminMeshStatus, updateAdminMeshConfig } from "../api/adminMesh";
import { BackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { ToastProvider } from "../components/Toast";
import { demoMeshStatus } from "../demo/DemoSystemStatusPage";
import { createQueryClient } from "../queryClient";
import { clearAllMeshMutationLocksForTests } from "../utils/meshMutationLock";
import { ServiceConfigPage } from "./ServiceConfigPage";

vi.mock("../api/adminConfig");
vi.mock("../api/adminMesh", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../api/adminMesh")>();
	return {
		...actual,
		fetchAdminMeshStatus: vi.fn(),
		updateAdminMeshConfig: vi.fn(),
	};
});
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
	vi.stubGlobal("navigator", {
		locks: {
			request: async (
				_name: string,
				_options: LockOptions,
				callback: (lock: Lock | null) => Promise<unknown>,
			) => callback({ name: fixtureCatalog.cluster.fixture53() } as Lock),
		},
	});
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
	vi.mocked(fetchAdminMeshStatus).mockResolvedValue({
		...demoMeshStatus,
		cluster_mesh_enabled: false,
	});
	vi.mocked(updateAdminMeshConfig).mockResolvedValue({ enabled: true });
}

describe("ServiceConfigPage", () => {
	beforeEach(() => {
		setupMocks();
	});

	afterEach(() => {
		cleanup();
		clearAllMeshMutationLocksForTests();
		vi.unstubAllGlobals();
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

	it("requires confirmation and refreshes the observed value after enabling Mesh", async () => {
		const user = userEvent.setup();
		renderPage();
		const toggle = await screen.findByRole("switch", {
			name: "Enable Reality Direct Mesh",
		});
		await user.click(toggle);
		await screen.findByRole("alertdialog", {
			name: "Enable Reality Direct Mesh?",
		});
		await user.click(screen.getByRole("button", { name: "Enable" }));

		await waitFor(() =>
			expect(updateAdminMeshConfig).toHaveBeenCalledWith("admintoken", true),
		);
		expect(fetchAdminMeshStatus).toHaveBeenCalledTimes(4);
		expect(screen.getByText("Mutation result unknown")).toBeInTheDocument();
	});

	it("shows directed preflight failures without exposing transport details", async () => {
		const user = userEvent.setup();
		vi.mocked(updateAdminMeshConfig).mockRejectedValueOnce(
			new BackendApiError({
				status: 409,
				message: "mesh preflight failed",
				code: "mesh_preflight_failed",
				details: {
					failures: [
						{
							sender_node_id: demoMeshStatus.local.node_id,
							target_node_id:
								demoMeshStatus.peers[0]?.node_id ?? "peer-unknown",
							kind: "protocol",
						},
					],
				},
			}),
		);
		renderPage();
		await user.click(
			await screen.findByRole("switch", {
				name: "Enable Reality Direct Mesh",
			}),
		);
		await user.click(await screen.findByRole("button", { name: "Enable" }));

		await waitFor(() =>
			expect(
				screen.getByText(/Directed cluster path validation failed/),
			).toBeInTheDocument(),
		);
		expect(screen.getByRole("alert")).not.toHaveTextContent(
			/socket|certificate|https?:\/\//i,
		);
	});

	it("keeps an unknown mutation locked across observation refresh", async () => {
		const user = userEvent.setup();
		vi.mocked(updateAdminMeshConfig).mockRejectedValueOnce(
			new Error("connection interrupted"),
		);
		renderPage();
		await user.click(
			await screen.findByRole("switch", {
				name: "Enable Reality Direct Mesh",
			}),
		);
		await user.click(await screen.findByRole("button", { name: "Enable" }));

		await waitFor(() =>
			expect(screen.getByText("Mutation result unknown")).toBeInTheDocument(),
		);
		expect(
			screen.getByRole("switch", {
				name: "Enable Reality Direct Mesh",
			}),
		).toBeDisabled();
		await user.click(
			screen.getByRole("button", { name: "Refresh observation" }),
		);
		await waitFor(() =>
			expect(screen.getByText("Mutation result unknown")).toBeInTheDocument(),
		);
		expect(updateAdminMeshConfig).toHaveBeenCalledTimes(1);
	});

	it("keeps an unknown mutation locked when observation reaches the target", async () => {
		const user = userEvent.setup();
		vi.mocked(updateAdminMeshConfig).mockRejectedValueOnce(
			new Error("connection interrupted"),
		);
		const observedValues = [false, false, false, true];
		vi.mocked(fetchAdminMeshStatus).mockReset();
		vi.mocked(fetchAdminMeshStatus).mockImplementation(async () => ({
			...demoMeshStatus,
			cluster_mesh_enabled: observedValues.shift() ?? true,
		}));

		renderPage();
		await user.click(
			await screen.findByRole("switch", {
				name: "Enable Reality Direct Mesh",
			}),
		);
		await user.click(await screen.findByRole("button", { name: "Enable" }));

		await waitFor(() =>
			expect(screen.getByText("Mutation result unknown")).toBeInTheDocument(),
		);
		await user.click(
			screen.getByRole("button", { name: "Refresh observation" }),
		);
		await waitFor(() =>
			expect(screen.getByText("Mutation result unknown")).toBeInTheDocument(),
		);
		expect(
			screen.getByRole("switch", {
				name: "Enable Reality Direct Mesh",
			}),
		).toBeDisabled();
	});

	it("does not submit after canceling the confirmation refresh", async () => {
		const user = userEvent.setup();
		let resolveRefresh!: (value: typeof demoMeshStatus) => void;
		const pendingRefresh = new Promise<typeof demoMeshStatus>((resolve) => {
			resolveRefresh = resolve;
		});
		vi.mocked(fetchAdminMeshStatus).mockReset();
		vi.mocked(fetchAdminMeshStatus)
			.mockResolvedValueOnce({ ...demoMeshStatus, cluster_mesh_enabled: false })
			.mockResolvedValueOnce({ ...demoMeshStatus, cluster_mesh_enabled: false })
			.mockImplementationOnce(() => pendingRefresh);

		renderPage();
		await user.click(
			await screen.findByRole("switch", {
				name: "Enable Reality Direct Mesh",
			}),
		);
		await user.click(await screen.findByRole("button", { name: "Enable" }));
		await user.click(screen.getByRole("button", { name: "Cancel" }));
		resolveRefresh({ ...demoMeshStatus, cluster_mesh_enabled: false });

		await waitFor(() =>
			expect(
				screen.queryByRole("alertdialog", {
					name: "Enable Reality Direct Mesh?",
				}),
			).toBeNull(),
		);
		expect(updateAdminMeshConfig).not.toHaveBeenCalled();
	});

	it("does not submit after canceling while acquiring the mutation lock", async () => {
		const user = userEvent.setup();
		let lockRequested!: () => void;
		let releaseLock!: () => void;
		const lockRequestStarted = new Promise<void>((resolve) => {
			lockRequested = resolve;
		});
		const lockRelease = new Promise<void>((resolve) => {
			releaseLock = resolve;
		});
		vi.stubGlobal("navigator", {
			locks: {
				request: async (
					_name: string,
					_options: LockOptions,
					callback: (lock: Lock | null) => Promise<unknown>,
				) => {
					lockRequested();
					await lockRelease;
					return callback({ name: fixtureCatalog.cluster.fixture53() } as Lock);
				},
			},
		});

		renderPage();
		await user.click(
			await screen.findByRole("switch", {
				name: "Enable Reality Direct Mesh",
			}),
		);
		await user.click(await screen.findByRole("button", { name: "Enable" }));
		await lockRequestStarted;
		await user.click(screen.getByRole("button", { name: "Cancel" }));
		releaseLock();

		await waitFor(() =>
			expect(
				screen.queryByRole("alertdialog", {
					name: "Enable Reality Direct Mesh?",
				}),
			).toBeNull(),
		);
		expect(updateAdminMeshConfig).not.toHaveBeenCalled();
	});
});
