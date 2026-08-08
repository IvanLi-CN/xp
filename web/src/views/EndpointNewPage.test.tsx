import { QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
	within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminConfig } from "../api/adminConfig";
import { createAdminEndpoint } from "../api/adminEndpoints";
import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { ToastProvider } from "../components/Toast";
import { createQueryClient } from "../queryClient";
import { EndpointNewPage } from "./EndpointNewPage";

const { mockNavigate, mockReadAdminToken } = vi.hoisted(() => ({
	mockNavigate: vi.fn(),
	mockReadAdminToken: vi.fn(() => "admintoken"),
}));

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
		useNavigate: () => mockNavigate,
	};
});

vi.mock("../api/adminEndpoints");
vi.mock("../api/adminNodes");
vi.mock("../api/adminConfig");

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
				<EndpointNewPage />
			</ToastProvider>
		</QueryClientProvider>,
	);
}

function setupNodeMocks() {
	vi.mocked(fetchAdminNodes).mockResolvedValue({
		items: [
			{
				node_id: "node-alpha",
				node_name: "alpha",
				access_host: "node-xp.example.test",
				api_base_url: "https://node-xp.example.test:443",
				quota_limit_bytes: 0,
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: null,
				},
			},
		],
	});
	vi.mocked(fetchAdminEndpoints).mockResolvedValue({
		items: [
			{
				endpoint_id: "endpoint-existing",
				node_id: "node-alpha",
				tag: "managed-alpha",
				kind: "vless_reality_vision_tcp",
				port: 443,
				meta: {
					reality: {
						dest: "127.0.0.1:39043",
						server_names: ["node-xp.example.test"],
						server_names_source: "manual",
						fingerprint: "chrome",
					},
					managed_default: true,
					canary_upstream: {
						url: "http://127.0.0.1:8080",
						mode: "auto",
					},
				},
			},
		],
	});
	vi.mocked(fetchAdminConfig).mockResolvedValue({
		bind: "127.0.0.1:62416",
		xray_api_addr: "127.0.0.1:10085",
		data_dir: "./data",
		node_name: "alpha",
		access_host: "node-xp.example.test",
		api_base_url: "https://node-xp.example.test:443",
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

describe("EndpointNewPage", () => {
	beforeEach(() => {
		mockNavigate.mockReset();
		mockReadAdminToken.mockReset();
		mockReadAdminToken.mockReturnValue("admintoken");
		vi.clearAllMocks();
		setupNodeMocks();
	});

	afterEach(() => {
		cleanup();
	});

	it("submits managed VLESS create payload without legacy reality fields", async () => {
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: "ep-managed",
			node_id: "node-alpha",
			tag: "ep-managed",
			kind: "vless_reality_vision_tcp",
			port: 443,
			meta: {
				managed_default: true,
			},
		});

		renderPage();

		fireEvent.change(await screen.findByLabelText("canaryUpstreamUrl"), {
			target: { value: " http://127.0.0.1:8080 " },
		});
		fireEvent.change(await screen.findByPlaceholderText("edge.example.com"), {
			target: { value: "Edge.Example.com." },
		});
		fireEvent.click(await screen.findByRole("button", { name: "Add" }));
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: "vless_reality_vision_tcp",
				node_id: "node-alpha",
				port: 443,
				canary_upstream: {
					url: "http://127.0.0.1:8080",
					mode: "auto",
				},
				accepted_authorities: ["edge.example.com:443"],
				mihomo_smux: {
					enabled: true,
					max_connections: 4,
					min_streams: 4,
					only_tcp: true,
				},
			});
		});
		expect(createAdminEndpoint).not.toHaveBeenCalledWith(
			"admintoken",
			expect.objectContaining({
				reality: expect.anything(),
			}),
		);
		await waitFor(() => {
			expect(mockNavigate).toHaveBeenCalledWith({
				to: "/endpoints/$endpointId",
				params: { endpointId: "ep-managed" },
			});
		});
	});

	it("restores managed autocomplete suggestions without changing create payload shape", async () => {
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: "ep-managed",
			node_id: "node-alpha",
			tag: "ep-managed",
			kind: "vless_reality_vision_tcp",
			port: 443,
			meta: {
				managed_default: true,
			},
		});

		renderPage();

		fireEvent.click(
			await screen.findByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			}),
		);
		fireEvent.click(
			await within(
				await screen.findByTestId("autocomplete-suggestions"),
			).findByText("https://127.0.0.1:39043"),
		);
		expect(await screen.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			"https://127.0.0.1:39043",
		);

		fireEvent.click(
			await screen.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		fireEvent.click(
			await within(
				await screen.findByTestId("tag-input-suggestions"),
			).findByText("node-xp.example.test"),
		);
		expect(
			await screen.findByText("node-xp.example.test:443"),
		).toBeInTheDocument();

		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: "vless_reality_vision_tcp",
				node_id: "node-alpha",
				port: 443,
				canary_upstream: {
					url: "https://127.0.0.1:39043",
					mode: "auto",
				},
				accepted_authorities: ["node-xp.example.test:443"],
				mihomo_smux: {
					enabled: true,
					max_connections: 4,
					min_streams: 4,
					only_tcp: true,
				},
			});
		});
	});

	it("keeps same-node XP HTTPS listener history ahead of the config fallback", async () => {
		vi.mocked(fetchAdminEndpoints).mockResolvedValue({
			items: [
				{
					endpoint_id: "endpoint-existing",
					node_id: "node-alpha",
					tag: "managed-alpha",
					kind: "vless_reality_vision_tcp",
					port: 443,
					meta: {
						reality: {
							dest: "127.0.0.1:49043",
							server_names: ["node-xp.example.test"],
							server_names_source: "manual",
							fingerprint: "chrome",
						},
						managed_default: true,
					},
				},
			],
		});
		vi.mocked(fetchAdminConfig).mockResolvedValue({
			bind: "127.0.0.1:62416",
			xray_api_addr: "127.0.0.1:10085",
			data_dir: "./data",
			node_name: "alpha",
			access_host: "node-xp.example.test",
			api_base_url: "https://node-xp.example.test:443",
			vless_https_canary_bind: "127.0.0.1:39043",
			quota_poll_interval_secs: 10,
			quota_auto_unban: true,
			ip_geo_enabled: false,
			ip_geo_origin: "https://api.country.is",
			mihomo_resource_allow_private_targets: false,
			admin_token_present: true,
			admin_token_masked: "********",
		});

		renderPage();

		fireEvent.click(
			await screen.findByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			}),
		);
		const suggestionPanel = await screen.findByTestId(
			"autocomplete-suggestions",
		);
		expect(
			within(suggestionPanel)
				.getAllByText(/^https:\/\/127\.0\.0\.1:/)
				.map((element) => element.textContent),
		).toEqual(["https://127.0.0.1:49043", "https://127.0.0.1:39043"]);

		fireEvent.click(
			within(suggestionPanel).getByText("https://127.0.0.1:49043"),
		);
		expect(await screen.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			"https://127.0.0.1:49043",
		);
	});

	it(
		"derives access-host suggestions from the selected endpoint port",
		{ timeout: 10_000 },
		async () => {
			vi.mocked(createAdminEndpoint).mockResolvedValue({
				endpoint_id: "ep-managed-8443",
				node_id: "node-alpha",
				tag: "ep-managed-8443",
				kind: "vless_reality_vision_tcp",
				port: 8443,
				meta: {
					managed_default: true,
				},
			});

			renderPage();

			fireEvent.change(await screen.findByLabelText("port"), {
				target: { value: "8443" },
			});
			fireEvent.click(
				await screen.findByRole("button", {
					name: "Show access host suggestions",
				}),
			);
			fireEvent.click(
				await within(
					await screen.findByTestId("tag-input-suggestions"),
				).findByText("node-xp.example.test:8443"),
			);
			expect(
				await screen.findByText("node-xp.example.test:8443"),
			).toBeInTheDocument();

			fireEvent.click(
				await screen.findByRole("button", { name: "Create endpoint" }),
			);

			await waitFor(() => {
				expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
					kind: "vless_reality_vision_tcp",
					node_id: "node-alpha",
					port: 8443,
					accepted_authorities: ["node-xp.example.test:8443"],
					mihomo_smux: {
						enabled: true,
						max_connections: 4,
						min_streams: 4,
						only_tcp: true,
					},
				});
			});
		},
	);

	it([
		"keeps the XP HTTPS listener suggestion available",
		"even without managed upstream history",
	].join(" "), async () => {
		vi.mocked(fetchAdminNodes).mockResolvedValue({
			items: [
				{
					node_id: "node-hinet",
					node_name: "hinet",
					access_host: "hinet-ep.707979.xyz",
					api_base_url: "https://hinet-xp.707979.xyz",
					quota_limit_bytes: 0,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
				},
			],
		});
		vi.mocked(fetchAdminEndpoints).mockResolvedValue({ items: [] });
		vi.mocked(fetchAdminConfig).mockResolvedValue({
			bind: "127.0.0.1:62416",
			xray_api_addr: "127.0.0.1:10085",
			data_dir: "./data",
			node_name: "hinet",
			access_host: "hinet-ep.707979.xyz",
			api_base_url: "https://hinet-xp.707979.xyz",
			vless_https_canary_bind: "127.0.0.1:39043",
			quota_poll_interval_secs: 10,
			quota_auto_unban: true,
			ip_geo_enabled: false,
			ip_geo_origin: "https://api.country.is",
			mihomo_resource_allow_private_targets: false,
			admin_token_present: true,
			admin_token_masked: "********",
		});
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: "ep-hinet-managed",
			node_id: "node-hinet",
			tag: "ep-hinet-managed",
			kind: "vless_reality_vision_tcp",
			port: 443,
			meta: {
				managed_default: true,
			},
		});

		renderPage();

		fireEvent.click(
			await screen.findByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			}),
		);
		fireEvent.click(
			await within(
				await screen.findByTestId("autocomplete-suggestions"),
			).findByText("https://127.0.0.1:39043"),
		);
		expect(await screen.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			"https://127.0.0.1:39043",
		);

		fireEvent.click(
			await screen.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		fireEvent.click(
			await within(
				await screen.findByTestId("tag-input-suggestions"),
			).findByText("hinet-ep.707979.xyz"),
		);
		expect(
			await screen.findByText("hinet-ep.707979.xyz:443"),
		).toBeInTheDocument();

		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: "vless_reality_vision_tcp",
				node_id: "node-hinet",
				port: 443,
				canary_upstream: {
					url: "https://127.0.0.1:39043",
					mode: "auto",
				},
				accepted_authorities: ["hinet-ep.707979.xyz:443"],
				mihomo_smux: {
					enabled: true,
					max_connections: 4,
					min_streams: 4,
					only_tcp: true,
				},
			});
		});
	});

	it("shows the backend error when managed create fails", async () => {
		vi.mocked(createAdminEndpoint).mockRejectedValue(
			new Error("400 invalid_request: invalid canary upstream"),
		);

		renderPage();

		fireEvent.change(await screen.findByLabelText("canaryUpstreamUrl"), {
			target: { value: "http://127.0.0.1:8080" },
		});
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		expect(
			await screen.findByText("400 invalid_request: invalid canary upstream"),
		).toBeInTheDocument();
		expect(createAdminEndpoint).toHaveBeenCalledTimes(1);
	});

	it("submits the editable Mihomo SMux policy for SS2022", async () => {
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: "ep-smux",
			node_id: "node-alpha",
			tag: "ep-smux",
			kind: "ss2022_2022_blake3_aes_128_gcm",
			port: 443,
			meta: {},
		});

		renderPage();
		fireEvent.click(await screen.findByLabelText("Kind"));
		fireEvent.click(
			await screen.findByRole("option", { name: "SS2022 BLAKE3 AES-128-GCM" }),
		);
		fireEvent.change(await screen.findByLabelText("最大物理连接数"), {
			target: { value: "8" },
		});
		fireEvent.change(await screen.findByLabelText("扩容前最小流数"), {
			target: { value: "6" },
		});
		fireEvent.click(await screen.findByLabelText("仅复用 TCP"));
		fireEvent.click(await screen.findByLabelText("启用 SMux"));
		expect(await screen.findByLabelText("最大物理连接数")).toBeDisabled();
		expect(await screen.findByLabelText("仅复用 TCP")).toBeDisabled();
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: "ss2022_2022_blake3_aes_128_gcm",
				node_id: "node-alpha",
				port: 443,
				mihomo_smux: {
					enabled: false,
					max_connections: 8,
					min_streams: 6,
					only_tcp: false,
				},
			});
		});
	});

	it("hides managed VLESS-only fields when switched to SS2022", async () => {
		renderPage();

		expect(
			await screen.findByLabelText("canaryUpstreamUrl"),
		).toBeInTheDocument();
		expect(
			await screen.findByLabelText("accepted host[:port]"),
		).toBeInTheDocument();

		fireEvent.click(await screen.findByLabelText("Kind"));
		fireEvent.click(
			await screen.findByRole("option", { name: "SS2022 BLAKE3 AES-128-GCM" }),
		);

		await waitFor(() => {
			expect(screen.queryByLabelText("canaryUpstreamUrl")).toBeNull();
			expect(screen.queryByLabelText("accepted host[:port]")).toBeNull();
		});
	});

	it([
		"keeps the XP HTTPS listener suggestion",
		"when only access-host suggestions are invalid",
	].join(" "), async () => {
		vi.mocked(fetchAdminNodes).mockResolvedValue({
			items: [
				{
					node_id: "node-alpha",
					node_name: "alpha",
					access_host: "",
					api_base_url: "not-a-url",
					quota_limit_bytes: 0,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
				},
			],
		});
		vi.mocked(fetchAdminEndpoints).mockResolvedValue({ items: [] });
		vi.mocked(fetchAdminConfig).mockResolvedValue({
			bind: "127.0.0.1:62416",
			xray_api_addr: "127.0.0.1:10085",
			data_dir: "./data",
			node_name: "alpha",
			access_host: "",
			api_base_url: "not-a-url",
			vless_https_canary_bind: "127.0.0.1:39043",
			quota_poll_interval_secs: 10,
			quota_auto_unban: true,
			ip_geo_enabled: false,
			ip_geo_origin: "https://api.country.is",
			mihomo_resource_allow_private_targets: false,
			admin_token_present: true,
			admin_token_masked: "********",
		});

		renderPage();

		await screen.findByLabelText("canaryUpstreamUrl");
		expect(
			screen.getByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			}),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", {
				name: "Show access host suggestions",
			}),
		).toBeNull();
	});
});
