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
		useParams: () => ({ endpointId: "endpoint-managed-vless" }),
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
}: { smuxSupported?: boolean } = {}) {
	const queryClient = createQueryClient();
	const compatibility = resolveApiCompatibility({
		capabilities: [
			...API_CAPABILITIES,
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
				node_id: "node-1",
				node_name: "tokyo-1",
				access_host: "edge.example.test",
				api_base_url: "https://tokyo-1.example.com",
				quota_limit_bytes: 0,
				quota_reset: {
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: null,
				},
			},
		],
	});
	vi.mocked(fetchAdminEndpoint).mockResolvedValue({
		endpoint_id: "endpoint-managed-vless",
		node_id: "node-1",
		tag: "managed-vless",
		kind: "vless_reality_vision_tcp",
		port: 53844,
		meta: {
			reality: {
				dest: "127.0.0.1:39043",
				server_names: ["edge.example.test"],
				server_names_source: "manual",
				fingerprint: "chrome",
			},
			managed_default: true,
			canary_upstream: {
				url: "http://127.0.0.1:8080",
				mode: "auto",
			},
			accepted_authorities: [
				"endpoint.example.test:53844",
				"edge.example.com:53844",
			],
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
			endpoint_id: "endpoint-managed-vless",
			node_id: "node-1",
			tag: "managed-vless",
			kind: "vless_reality_vision_tcp",
			port: 53844,
			meta: {
				reality: {
					dest: "127.0.0.1:39043",
					server_names: ["edge.example.test"],
					server_names_source: "manual",
					fingerprint: "chrome",
				},
				managed_default: true,
				canary_upstream: {
					url: "https://tokyo-1.example.com",
					mode: "auto",
				},
				accepted_authorities: [
					"endpoint.example.test:53844",
					"edge.example.com:53844",
					"edge.example.test:53844",
				],
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
			).findByText("edge.example.test:53844"),
		);
		expect(
			await screen.findByTitle("edge.example.test:53844"),
		).toBeInTheDocument();

		fireEvent.click(
			await screen.findByRole("button", { name: "Save changes" }),
		);

		await waitFor(() => {
			expect(patchAdminEndpoint).toHaveBeenCalledWith(
				"admintoken",
				"endpoint-managed-vless",
				{
					port: 53844,
					canary_upstream: {
						url: "https://127.0.0.1:39043",
						mode: "auto",
					},
					accepted_authorities: [
						"endpoint.example.test:53844",
						"edge.example.com:53844",
						"edge.example.test:53844",
					],
				},
			);
		});
	});

	it("does not render or submit SMux settings for VLESS endpoints", async () => {
		vi.mocked(patchAdminEndpoint).mockResolvedValue({
			endpoint_id: "endpoint-managed-vless",
			node_id: "node-1",
			tag: "managed-vless",
			kind: "vless_reality_vision_tcp",
			port: 53844,
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
				"endpoint-managed-vless",
				{ port: 53844 },
			);
		});
	});

	it("does not render or submit SMux settings to a legacy endpoint API", async () => {
		vi.mocked(fetchAdminEndpoint).mockResolvedValue({
			endpoint_id: "endpoint-managed-vless",
			node_id: "node-1",
			tag: "legacy-ss2022",
			kind: "ss2022_2022_blake3_aes_128_gcm",
			port: 53844,
			meta: {},
		});
		vi.mocked(patchAdminEndpoint).mockResolvedValue({
			endpoint_id: "endpoint-managed-vless",
			node_id: "node-1",
			tag: "legacy-ss2022",
			kind: "ss2022_2022_blake3_aes_128_gcm",
			port: 53844,
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
				"endpoint-managed-vless",
				{ port: 53844 },
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
			vi.mocked(fetchAdminEndpoint).mockResolvedValue({
				endpoint_id: "endpoint-managed-vless",
				node_id: "node-hinet",
				tag: "managed-hinet",
				kind: "vless_reality_vision_tcp",
				port: 53844,
				meta: {
					reality: {
						dest: "127.0.0.1:39043",
						server_names: ["hinet-ep.707979.xyz"],
						server_names_source: "manual",
						fingerprint: "chrome",
					},
					managed_default: true,
				},
			});
			vi.mocked(patchAdminEndpoint).mockResolvedValue({
				endpoint_id: "endpoint-managed-vless",
				node_id: "node-hinet",
				tag: "managed-hinet",
				kind: "vless_reality_vision_tcp",
				port: 53844,
				meta: {
					reality: {
						dest: "127.0.0.1:39043",
						server_names: ["hinet-ep.707979.xyz"],
						server_names_source: "manual",
						fingerprint: "chrome",
					},
					managed_default: true,
					canary_upstream: {
						url: "https://127.0.0.1:39043",
						mode: "auto",
					},
					accepted_authorities: ["hinet-ep.707979.xyz:53844"],
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
				).findByText("hinet-ep.707979.xyz:53844"),
			);
			expect(
				await screen.findByTitle("hinet-ep.707979.xyz:53844"),
			).toBeInTheDocument();

			fireEvent.click(
				await screen.findByRole("button", { name: "Save changes" }),
			);

			await waitFor(() => {
				expect(patchAdminEndpoint).toHaveBeenCalledWith(
					"admintoken",
					"endpoint-managed-vless",
					{
						port: 53844,
						canary_upstream: {
							url: "https://127.0.0.1:39043",
							mode: "auto",
						},
						accepted_authorities: ["hinet-ep.707979.xyz:53844"],
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
					node_id: "node-1",
					node_name: "tokyo-1",
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
