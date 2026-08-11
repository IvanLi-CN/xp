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

import { fetchAdminEndpoint, patchAdminEndpoint } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { resolveApiCompatibility } from "../api/apiCompatibility";
import { API_CAPABILITIES } from "../api/releaseInventories";
import { ApiCompatibilityProvider } from "../api/useApiCompatibility";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { EndpointDetailsPage } from "./EndpointDetailsPage";

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
		useParams: () => ({ endpointId: fixtureCatalog.endpointId.fixture172() }),
	};
});

vi.mock("../api/adminEndpoints", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../api/adminEndpoints")>();
	return {
		...actual,
		fetchAdminEndpoint: vi.fn(),
		patchAdminEndpoint: vi.fn(),
	};
});
vi.mock("../api/adminNodes");

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
				<UiPrefsProvider>
					<ToastProvider>
						<EndpointDetailsPage />
					</ToastProvider>
				</UiPrefsProvider>
			</ApiCompatibilityProvider>
		</QueryClientProvider>,
	);
}

function setupMocks() {
	vi.mocked(fetchAdminNodes).mockResolvedValue({
		items: [
			{
				node_id: fixtureCatalog.nodeId.fixture32(),
				node_name: fixtureCatalog.nodeName.fixture33(),
				access_host: fixtureCatalog.host.fixture119(),
				api_base_url: fixtureCatalog.service.fixture34(),
				quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
				quota_reset: fixtureCatalog.quota.resetNode(),
			},
		],
	});
	vi.mocked(fetchAdminEndpoint).mockResolvedValue({
		endpoint_id: fixtureCatalog.endpointId.fixture172(),
		node_id: fixtureCatalog.nodeId.fixture32(),
		tag: fixtureCatalog.endpointTag.fixture121(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: fixtureCatalog.endpoint.port53844(),
		meta: {
			reality: {
				dest: fixtureCatalog.address.loopback39043(),
				server_names: fixtureCatalog.hostList.edge5(),
				server_names_source: "manual",
				fingerprint: "chrome",
			},
			managed_default: true,
			canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
			accepted_authorities: fixtureCatalog.hostList.edge12(),
		},
	});
}

describe("EndpointDetailsPage", () => {
	beforeEach(() => {
		mockNavigate.mockReset();
		mockReadAdminToken.mockReset();
		mockReadAdminToken.mockReturnValue("admintoken");
		vi.clearAllMocks();
		setupMocks();
	});

	afterEach(() => {
		cleanup();
	});

	it("restores managed suggestions and keeps the managed patch payload stable", async () => {
		vi.mocked(patchAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture172(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			tag: fixtureCatalog.endpointTag.fixture121(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port53844(),
			meta: {
				reality: {
					dest: fixtureCatalog.address.loopback39043(),
					server_names: fixtureCatalog.hostList.edge5(),
					server_names_source: "manual",
					fingerprint: "chrome",
				},
				managed_default: true,
				canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
				accepted_authorities: fixtureCatalog.hostList.edge13(),
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
		expect(await screen.findByLabelText("canary upstream url")).toHaveValue(
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
			).findByText(fixtureCatalog.authority.host119Port53844()[0]),
		);
		expect(
			await screen.findByTitle(fixtureCatalog.authority.host119Port53844()[0]),
		).toBeInTheDocument();

		fireEvent.click(
			await screen.findByRole("button", { name: "Save changes" }),
		);

		await waitFor(() => {
			expect(patchAdminEndpoint).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.endpointId.fixture172(),
				{
					port: fixtureCatalog.endpoint.port53844(),
					canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
					accepted_authorities:
						fixtureCatalog.authority.existingAndHost119Port53844(),
				},
			);
		});
	});

	it("falls back missing legacy transport metadata and patches an XHTTP switch", async () => {
		vi.mocked(fetchAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture172(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			tag: fixtureCatalog.endpointTag.fixture121(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port53844(),
			meta: {
				reality: {
					dest: fixtureCatalog.address.loopback39043(),
					server_names: fixtureCatalog.hostList.edge5(),
					server_names_source: "manual",
					fingerprint: "chrome",
				},
				managed_default: false,
			},
		});
		vi.mocked(patchAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture172(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			tag: fixtureCatalog.endpointTag.fixture121(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port53844(),
			meta: { transport: "xhttp" },
		});

		renderPage();
		fireEvent.click(await screen.findByText("Advanced: VLESS transport"));
		expect(
			await screen.findByRole("radio", { name: "Vision TCP" }),
		).toBeChecked();
		fireEvent.click(await screen.findByRole("radio", { name: "XHTTP / XMUX" }));
		expect(
			await screen.findByText(/Changing this mode rebuilds the inbound/),
		).toBeInTheDocument();
		fireEvent.click(
			await screen.findByRole("button", { name: "Save changes" }),
		);

		await waitFor(() => {
			expect(patchAdminEndpoint).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.endpointId.fixture172(),
				{
					port: fixtureCatalog.endpoint.port53844(),
					transport: "xhttp",
				},
			);
		});
	});

	it("switches XHTTP back to Vision TCP and keeps the choice after an API error", async () => {
		vi.mocked(fetchAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture172(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			tag: fixtureCatalog.endpointTag.fixture121(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port53844(),
			meta: {
				reality: {
					dest: fixtureCatalog.address.loopback39043(),
					server_names: fixtureCatalog.hostList.edge5(),
					server_names_source: "manual",
					fingerprint: "chrome",
				},
				managed_default: false,
				transport: "xhttp",
			},
		});
		vi.mocked(patchAdminEndpoint).mockRejectedValue(
			new Error("409 inbound rebuild rejected"),
		);

		renderPage();
		fireEvent.click(await screen.findByText("Advanced: VLESS transport"));
		expect(
			await screen.findByRole("radio", { name: "XHTTP / XMUX" }),
		).toBeChecked();
		fireEvent.click(await screen.findByRole("radio", { name: "Vision TCP" }));
		fireEvent.click(
			await screen.findByRole("button", { name: "Save changes" }),
		);

		await waitFor(() => {
			expect(patchAdminEndpoint).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.endpointId.fixture172(),
				{
					port: fixtureCatalog.endpoint.port53844(),
					transport: "vision_tcp",
				},
			);
		});
		expect(
			await screen.findByText("409 inbound rebuild rejected"),
		).toBeVisible();
		expect(
			await screen.findByRole("radio", { name: "Vision TCP" }),
		).toBeChecked();
	});

	it("hides VLESS transport controls when the API capability is unavailable", async () => {
		renderPage({ xhttpSupported: false });

		expect(await screen.findByLabelText("port")).toBeInTheDocument();
		expect(
			screen.queryByText("Advanced: VLESS transport"),
		).not.toBeInTheDocument();
		fireEvent.click(
			await screen.findByRole("button", { name: "Save changes" }),
		);

		await waitFor(() => {
			expect(patchAdminEndpoint).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.endpointId.fixture172(),
				{
					port: fixtureCatalog.endpoint.port53844(),
					accepted_authorities:
						fixtureCatalog.authority.existingAuthoritiesPort443(),
				},
			);
		});
	});

	it("does not render or submit SMux settings for VLESS endpoints", async () => {
		vi.mocked(patchAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture172(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			tag: fixtureCatalog.endpointTag.fixture121(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port53844(),
			meta: {},
		});

		renderPage();
		expect(await screen.findByLabelText("port")).toBeInTheDocument();
		expect(screen.queryByLabelText("启用 SMux")).toBeNull();
		fireEvent.click(
			await screen.findByRole("button", { name: "Save changes" }),
		);

		await waitFor(() => {
			expect(patchAdminEndpoint).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.endpointId.fixture172(),
				{
					port: fixtureCatalog.endpoint.port53844(),
					accepted_authorities:
						fixtureCatalog.authority.existingAuthoritiesPort443(),
				},
			);
		});
	});

	it("does not render or submit SMux settings to a legacy endpoint API", async () => {
		vi.mocked(fetchAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture172(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			tag: fixtureCatalog.endpointTag.fixture121(),
			kind: fixtureCatalog.endpoint.ssKind(),
			port: fixtureCatalog.endpoint.port53844(),
			meta: {},
		});
		vi.mocked(patchAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture172(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			tag: fixtureCatalog.endpointTag.fixture121(),
			kind: fixtureCatalog.endpoint.ssKind(),
			port: fixtureCatalog.endpoint.port53844(),
			meta: {},
		});

		renderPage({ smuxSupported: false });

		expect(
			await screen.findByText(
				"This server does not support per-endpoint Mihomo SMux settings.",
			),
		).toBeInTheDocument();
		expect(screen.queryByLabelText("启用 SMux")).toBeNull();
		fireEvent.click(
			await screen.findByRole("button", { name: "Save changes" }),
		);

		await waitFor(() => {
			expect(patchAdminEndpoint).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.endpointId.fixture172(),
				{ port: fixtureCatalog.endpoint.port53844() },
			);
		});
	});

	it(
		[
			"keeps the XP HTTPS listener suggestion available",
			"when the managed endpoint has no canary history",
		].join(" "),
		{ timeout: 20_000 },
		async () => {
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
			vi.mocked(fetchAdminEndpoint).mockResolvedValue({
				endpoint_id: fixtureCatalog.endpointId.fixture172(),
				node_id: fixtureCatalog.nodeId.fixture124(),
				tag: fixtureCatalog.endpointTag.fixture129(),
				kind: fixtureCatalog.endpoint.vlessKind(),
				port: fixtureCatalog.endpoint.port53844(),
				meta: {
					reality: {
						dest: fixtureCatalog.address.loopback39043(),
						server_names: fixtureCatalog.hostList.edge7(),
						server_names_source: "manual",
						fingerprint: "chrome",
					},
					managed_default: true,
				},
			});
			vi.mocked(patchAdminEndpoint).mockResolvedValue({
				endpoint_id: fixtureCatalog.endpointId.fixture172(),
				node_id: fixtureCatalog.nodeId.fixture124(),
				tag: fixtureCatalog.endpointTag.fixture129(),
				kind: fixtureCatalog.endpoint.vlessKind(),
				port: fixtureCatalog.endpoint.port53844(),
				meta: {
					reality: {
						dest: fixtureCatalog.address.loopback39043(),
						server_names: fixtureCatalog.hostList.edge7(),
						server_names_source: "manual",
						fingerprint: "chrome",
					},
					managed_default: true,
					canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
					accepted_authorities: fixtureCatalog.hostList.edge15(),
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
			expect(await screen.findByLabelText("canary upstream url")).toHaveValue(
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
				).findByText(fixtureCatalog.authority.host126Port53844()[0]),
			);
			expect(
				await screen.findByTitle(
					fixtureCatalog.authority.host126Port53844()[0],
				),
			).toBeInTheDocument();

			fireEvent.click(
				await screen.findByRole("button", { name: "Save changes" }),
			);

			await waitFor(() => {
				expect(patchAdminEndpoint).toHaveBeenCalledWith(
					"admintoken",
					fixtureCatalog.endpointId.fixture172(),
					{
						port: fixtureCatalog.endpoint.port53844(),
						canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
						accepted_authorities: fixtureCatalog.authority.host126Port53844(),
					},
				);
			});
		},
	);

	it([
		"keeps the XP HTTPS listener suggestion",
		"when endpoint node access-host data is invalid",
	].join(" "), async () => {
		vi.mocked(fetchAdminNodes).mockResolvedValue({
			items: [
				{
					node_id: fixtureCatalog.nodeId.fixture32(),
					node_name: fixtureCatalog.nodeName.fixture33(),
					access_host: fixtureCatalog.string.none(),
					api_base_url: fixtureCatalog.service.fixture123(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.resetNode(),
				},
			],
		});

		renderPage();

		await screen.findByLabelText("canary upstream url");
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
