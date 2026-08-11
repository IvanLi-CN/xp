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
import { fixtureCatalog } from "../fixture-policy/catalog";

import { fetchAdminConfig } from "../api/adminConfig";
import { createAdminEndpoint } from "../api/adminEndpoints";
import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { resolveApiCompatibility } from "../api/apiCompatibility";
import { API_CAPABILITIES } from "../api/releaseInventories";
import { ApiCompatibilityProvider } from "../api/useApiCompatibility";
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

function renderPage({
	smuxSupported = true,
	xhttpSupported = true,
}: { smuxSupported?: boolean; xhttpSupported?: boolean } = {}) {
	const queryClient = createQueryClient();
	const compatibility = resolveApiCompatibility({
		capabilities: [
			...API_CAPABILITIES.filter(
				(capability) => capability !== "admin.endpoint-vless-xhttp",
			),
			...(xhttpSupported ? ["admin.endpoint-vless-xhttp"] : []),
			...(smuxSupported ? ["admin.endpoint-mihomo-smux"] : []),
		],
	});
	return render(
		<QueryClientProvider client={queryClient}>
			<ApiCompatibilityProvider value={compatibility}>
				<ToastProvider>
					<EndpointNewPage />
				</ToastProvider>
			</ApiCompatibilityProvider>
		</QueryClientProvider>,
	);
}

function setupNodeMocks() {
	vi.mocked(fetchAdminNodes).mockResolvedValue({
		items: [
			{
				node_id: fixtureCatalog.nodeId.fixture182(),
				node_name: fixtureCatalog.nodeName.fixture86(),
				access_host: fixtureCatalog.host.fixture130(),
				api_base_url: fixtureCatalog.service.fixture131(),
				quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
				quota_reset: fixtureCatalog.quota.resetNode(),
			},
		],
	});
	vi.mocked(fetchAdminEndpoints).mockResolvedValue({
		items: [
			{
				endpoint_id: fixtureCatalog.endpointId.fixture132(),
				node_id: fixtureCatalog.nodeId.fixture182(),
				tag: fixtureCatalog.endpointTag.fixture133(),
				kind: fixtureCatalog.endpoint.vlessKind(),
				port: fixtureCatalog.endpoint.port443(),
				meta: {
					reality: {
						dest: fixtureCatalog.address.loopback39043(),
						server_names: fixtureCatalog.hostList.edge8(),
						server_names_source: "manual",
						fingerprint: "chrome",
					},
					managed_default: true,
					canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
				},
			},
		],
	});
	vi.mocked(fetchAdminConfig).mockResolvedValue({
		bind: fixtureCatalog.address.loopbackPort39058(),
		xray_api_addr: fixtureCatalog.address.loopbackPort39059(),
		data_dir: "./data",
		node_name: fixtureCatalog.nodeName.fixture86(),
		access_host: fixtureCatalog.host.fixture130(),
		api_base_url: fixtureCatalog.service.fixture131(),
		vless_https_canary_bind: fixtureCatalog.address.loopback39043(),
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
			endpoint_id: fixtureCatalog.endpointId.fixture105(),
			node_id: fixtureCatalog.nodeId.fixture182(),
			tag: fixtureCatalog.endpointId.fixture105(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
			meta: {
				managed_default: true,
			},
		});

		renderPage();
		fireEvent.click(await screen.findByText("Advanced: VLESS transport"));
		expect(
			await screen.findByRole("radio", { name: "XHTTP / XMUX" }),
		).toBeChecked();
		expect(
			await screen.findByText(
				"Recommended. Mihomo YAML uses one reusable HTTP/2 connection after pool warm-up.",
			),
		).toBeInTheDocument();

		fireEvent.change(await screen.findByLabelText("canaryUpstreamUrl"), {
			target: { value: " http://127.0.0.1:8080 " },
		});
		fireEvent.change(await screen.findByPlaceholderText("edge.example.com"), {
			target: { value: fixtureCatalog.authority.edgeExamplePort443()[0] },
		});
		fireEvent.click(await screen.findByRole("button", { name: "Add" }));
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: fixtureCatalog.endpoint.vlessKind(),
				node_id: fixtureCatalog.nodeId.fixture182(),
				port: fixtureCatalog.endpoint.port443(),
				transport: "xhttp",
				canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
				accepted_authorities: fixtureCatalog.authority.edgeExamplePort443(),
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
				params: { endpointId: fixtureCatalog.endpointId.fixture105() },
			});
		});
	});

	it("omits SMux controls and payload for VLESS endpoints", async () => {
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture105(),
			node_id: fixtureCatalog.nodeId.fixture182(),
			tag: fixtureCatalog.endpointId.fixture105(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
			meta: { managed_default: true },
		});

		renderPage();

		expect(screen.queryByLabelText("启用 SMux")).toBeNull();
		expect(
			screen.queryByText(
				"This server does not support per-endpoint Mihomo SMux settings.",
			),
		).toBeNull();
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: fixtureCatalog.endpoint.vlessKind(),
				node_id: fixtureCatalog.nodeId.fixture182(),
				port: fixtureCatalog.endpoint.port443(),
				transport: "xhttp",
				canary_upstream: fixtureCatalog.optional.undefined(),
				accepted_authorities: fixtureCatalog.optional.undefined(),
			});
		});
	});

	it("hides VLESS transport and omits its field for a legacy endpoint API", async () => {
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture105(),
			node_id: fixtureCatalog.nodeId.fixture182(),
			tag: fixtureCatalog.endpointId.fixture105(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
			meta: { managed_default: true },
		});

		renderPage({ xhttpSupported: false });

		expect(
			screen.queryByText("Advanced: VLESS transport"),
		).not.toBeInTheDocument();
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: fixtureCatalog.endpoint.vlessKind(),
				node_id: fixtureCatalog.nodeId.fixture182(),
				port: fixtureCatalog.endpoint.port443(),
				canary_upstream: fixtureCatalog.optional.undefined(),
				accepted_authorities: fixtureCatalog.optional.undefined(),
			});
		});
	});

	it("restores managed autocomplete suggestions without changing create payload shape", async () => {
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture105(),
			node_id: fixtureCatalog.nodeId.fixture182(),
			tag: fixtureCatalog.endpointId.fixture105(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
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
			).findByText(fixtureCatalog.canaryUpstream.httpsListener().url),
		);
		expect(await screen.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			fixtureCatalog.canaryUpstream.httpsListener().url,
		);

		fireEvent.click(
			await screen.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		fireEvent.click(
			await within(
				await screen.findByTestId("tag-input-suggestions"),
			).findByText(fixtureCatalog.authority.host130()[0]),
		);
		expect(
			await screen.findByText(fixtureCatalog.authority.host130Port443()[0]),
		).toBeInTheDocument();

		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: fixtureCatalog.endpoint.vlessKind(),
				node_id: fixtureCatalog.nodeId.fixture182(),
				port: fixtureCatalog.endpoint.port443(),
				transport: "xhttp",
				canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
				accepted_authorities: fixtureCatalog.authority.host130Port443(),
			});
		});
	});

	it("keeps same-node XP HTTPS listener history ahead of the config fallback", async () => {
		vi.mocked(fetchAdminEndpoints).mockResolvedValue({
			items: [
				{
					endpoint_id: fixtureCatalog.endpointId.fixture132(),
					node_id: fixtureCatalog.nodeId.fixture182(),
					tag: fixtureCatalog.endpointTag.fixture133(),
					kind: fixtureCatalog.endpoint.vlessKind(),
					port: fixtureCatalog.endpoint.port443(),
					meta: {
						reality: {
							dest: fixtureCatalog.address.loopback49043(),
							server_names: fixtureCatalog.hostList.edge8(),
							server_names_source: "manual",
							fingerprint: "chrome",
						},
						managed_default: true,
					},
				},
			],
		});
		vi.mocked(fetchAdminConfig).mockResolvedValue({
			bind: fixtureCatalog.address.loopbackPort39058(),
			xray_api_addr: fixtureCatalog.address.loopbackPort39059(),
			data_dir: "./data",
			node_name: fixtureCatalog.nodeName.fixture86(),
			access_host: fixtureCatalog.host.fixture130(),
			api_base_url: fixtureCatalog.service.fixture131(),
			vless_https_canary_bind: fixtureCatalog.address.loopback39043(),
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
		).toEqual([
			fixtureCatalog.canaryUpstream.httpsAlternate().url,
			fixtureCatalog.canaryUpstream.httpsListener().url,
		]);

		fireEvent.click(
			within(suggestionPanel).getByText(
				fixtureCatalog.canaryUpstream.httpsAlternate().url,
			),
		);
		expect(await screen.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			fixtureCatalog.canaryUpstream.httpsAlternate().url,
		);
	});

	it(
		"derives access-host suggestions from the selected endpoint port",
		{ timeout: 10_000 },
		async () => {
			vi.mocked(createAdminEndpoint).mockResolvedValue({
				endpoint_id: fixtureCatalog.endpointId.fixture185(),
				node_id: fixtureCatalog.nodeId.fixture182(),
				tag: fixtureCatalog.endpointId.fixture185(),
				kind: fixtureCatalog.endpoint.vlessKind(),
				port: fixtureCatalog.endpoint.port8443(),
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
				).findByText(fixtureCatalog.authority.host130Port8443()[0]),
			);
			expect(
				await screen.findByText(fixtureCatalog.authority.host130Port8443()[0]),
			).toBeInTheDocument();

			fireEvent.click(
				await screen.findByRole("button", { name: "Create endpoint" }),
			);

			await waitFor(() => {
				expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
					kind: fixtureCatalog.endpoint.vlessKind(),
					node_id: fixtureCatalog.nodeId.fixture182(),
					port: fixtureCatalog.endpoint.port8443(),
					transport: "xhttp",
					accepted_authorities: fixtureCatalog.authority.host130Port8443(),
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
					node_id: fixtureCatalog.nodeId.fixture124(),
					node_name: fixtureCatalog.nodeName.fixture125(),
					access_host: fixtureCatalog.host.fixture126(),
					api_base_url: fixtureCatalog.service.fixture127(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.resetNode(),
				},
			],
		});
		vi.mocked(fetchAdminEndpoints).mockResolvedValue({ items: [] });
		vi.mocked(fetchAdminConfig).mockResolvedValue({
			bind: fixtureCatalog.address.loopbackPort39058(),
			xray_api_addr: fixtureCatalog.address.loopbackPort39059(),
			data_dir: "./data",
			node_name: fixtureCatalog.nodeName.fixture125(),
			access_host: fixtureCatalog.host.fixture126(),
			api_base_url: fixtureCatalog.service.fixture127(),
			vless_https_canary_bind: fixtureCatalog.address.loopback39043(),
			quota_poll_interval_secs: 10,
			quota_auto_unban: true,
			ip_geo_enabled: false,
			ip_geo_origin: "https://api.country.is",
			mihomo_resource_allow_private_targets: false,
			admin_token_present: true,
			admin_token_masked: "********",
		});
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture186(),
			node_id: fixtureCatalog.nodeId.fixture124(),
			tag: fixtureCatalog.endpointId.fixture186(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
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
			).findByText(fixtureCatalog.canaryUpstream.httpsListener().url),
		);
		expect(await screen.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			fixtureCatalog.canaryUpstream.httpsListener().url,
		);

		fireEvent.click(
			await screen.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		fireEvent.click(
			await within(
				await screen.findByTestId("tag-input-suggestions"),
			).findByText(fixtureCatalog.authority.host126()[0]),
		);
		expect(
			await screen.findByText(fixtureCatalog.authority.host126Port443()[0]),
		).toBeInTheDocument();

		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: fixtureCatalog.endpoint.vlessKind(),
				node_id: fixtureCatalog.nodeId.fixture124(),
				port: fixtureCatalog.endpoint.port443(),
				transport: "xhttp",
				canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
				accepted_authorities: fixtureCatalog.authority.host126Port443(),
			});
		});
	});

	it("shows the backend error when managed create fails", async () => {
		vi.mocked(createAdminEndpoint).mockRejectedValue(
			new Error("400 invalid_request: invalid canary upstream"),
		);

		renderPage();
		fireEvent.click(await screen.findByText("Advanced: VLESS transport"));
		fireEvent.click(await screen.findByRole("radio", { name: "Vision TCP" }));

		fireEvent.change(await screen.findByLabelText("canaryUpstreamUrl"), {
			target: { value: "http://127.0.0.1:8080" },
		});
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		expect(
			await screen.findByText("400 invalid_request: invalid canary upstream"),
		).toBeInTheDocument();
		expect(
			await screen.findByRole("radio", { name: "Vision TCP" }),
		).toBeChecked();
		expect(createAdminEndpoint).toHaveBeenCalledWith(
			"admintoken",
			expect.objectContaining({ transport: "vision_tcp" }),
		);
		expect(createAdminEndpoint).toHaveBeenCalledTimes(1);
	});

	it("submits the editable Mihomo SMux policy for SS2022", async () => {
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture105(),
			node_id: fixtureCatalog.nodeId.fixture182(),
			tag: fixtureCatalog.endpointId.fixture105(),
			kind: fixtureCatalog.endpoint.ssKind(),
			port: fixtureCatalog.endpoint.port443(),
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
				kind: fixtureCatalog.endpoint.ssKind(),
				node_id: fixtureCatalog.nodeId.fixture182(),
				port: fixtureCatalog.endpoint.port443(),
				mihomo_smux: {
					enabled: false,
					max_connections: 8,
					min_streams: 6,
					only_tcp: false,
				},
			});
		});
	});

	it("does not validate hidden SS2022 SMux values after switching to VLESS", async () => {
		vi.mocked(createAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture105(),
			node_id: fixtureCatalog.nodeId.fixture182(),
			tag: fixtureCatalog.endpointId.fixture105(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
			meta: {},
		});

		renderPage();
		fireEvent.click(await screen.findByLabelText("Kind"));
		fireEvent.click(
			await screen.findByRole("option", { name: "SS2022 BLAKE3 AES-128-GCM" }),
		);
		fireEvent.change(await screen.findByLabelText("最大物理连接数"), {
			target: { value: "17" },
		});
		fireEvent.click(await screen.findByLabelText("Kind"));
		fireEvent.click(
			await screen.findByRole("option", { name: "VLESS Reality" }),
		);
		fireEvent.click(
			await screen.findByRole("button", { name: "Create endpoint" }),
		);

		await waitFor(() => {
			expect(createAdminEndpoint).toHaveBeenCalledWith("admintoken", {
				kind: fixtureCatalog.endpoint.vlessKind(),
				node_id: fixtureCatalog.nodeId.fixture182(),
				port: fixtureCatalog.endpoint.port443(),
				transport: "xhttp",
				canary_upstream: fixtureCatalog.optional.undefined(),
				accepted_authorities: fixtureCatalog.optional.undefined(),
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
					node_id: fixtureCatalog.nodeId.fixture182(),
					node_name: fixtureCatalog.nodeName.fixture86(),
					access_host: fixtureCatalog.string.none(),
					api_base_url: fixtureCatalog.service.fixture123(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.resetNode(),
				},
			],
		});
		vi.mocked(fetchAdminEndpoints).mockResolvedValue({ items: [] });
		vi.mocked(fetchAdminConfig).mockResolvedValue({
			bind: fixtureCatalog.address.loopbackPort39058(),
			xray_api_addr: fixtureCatalog.address.loopbackPort39059(),
			data_dir: "./data",
			node_name: fixtureCatalog.nodeName.fixture86(),
			access_host: fixtureCatalog.string.none(),
			api_base_url: fixtureCatalog.service.fixture123(),
			vless_https_canary_bind: fixtureCatalog.address.loopback39043(),
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
