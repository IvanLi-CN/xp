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
		useParams: () => ({ endpointId: fixtureCatalog.slotString.s172() }),
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
				node_id: fixtureCatalog.slotString.s32(),
				node_name: fixtureCatalog.slotString.s33(),
				access_host: fixtureCatalog.slotString.s119(),
				api_base_url: fixtureCatalog.slotString.s34(),
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
		endpoint_id: fixtureCatalog.slotString.s172(),
		node_id: fixtureCatalog.slotString.s32(),
		tag: fixtureCatalog.slotString.s121(),
		kind: "vless_reality_vision_tcp",
		port: 53844,
		meta: {
			reality: {
				dest: fixtureCatalog.address.loopback39043(),
				server_names: fixtureCatalog.slotList.l5(),
				server_names_source: "manual",
				fingerprint: "chrome",
			},
			managed_default: true,
			canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
			accepted_authorities: fixtureCatalog.slotList.l12(),
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
			endpoint_id: fixtureCatalog.slotString.s172(),
			node_id: fixtureCatalog.slotString.s32(),
			tag: fixtureCatalog.slotString.s121(),
			kind: "vless_reality_vision_tcp",
			port: 53844,
			meta: {
				reality: {
					dest: fixtureCatalog.address.loopback39043(),
					server_names: fixtureCatalog.slotList.l5(),
					server_names_source: "manual",
					fingerprint: "chrome",
				},
				managed_default: true,
				canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
				accepted_authorities: fixtureCatalog.slotList.l13(),
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
				fixtureCatalog.slotString.s172(),
				{
					port: 53844,
					canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
					accepted_authorities:
						fixtureCatalog.authority.existingAndHost119Port53844(),
				},
			);
		});
	});

	it("defaults legacy endpoint SMux and persists an edited policy", async () => {
		vi.mocked(patchAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.slotString.s172(),
			node_id: fixtureCatalog.slotString.s32(),
			tag: fixtureCatalog.slotString.s121(),
			kind: "vless_reality_vision_tcp",
			port: 53844,
			meta: {},
		});

		renderPage();
		const enabled = await screen.findByLabelText("启用 SMux");
		expect(enabled).toHaveAttribute("data-state", "checked");
		fireEvent.change(await screen.findByLabelText("最大物理连接数"), {
			target: { value: "8" },
		});
		fireEvent.change(await screen.findByLabelText("扩容前最小流数"), {
			target: { value: "6" },
		});
		fireEvent.click(await screen.findByLabelText("仅复用 TCP"));
		fireEvent.click(enabled);
		expect(await screen.findByLabelText("最大物理连接数")).toBeDisabled();
		expect(await screen.findByLabelText("仅复用 TCP")).toBeDisabled();
		fireEvent.click(
			await screen.findByRole("button", { name: "Save changes" }),
		);

		await waitFor(() => {
			expect(patchAdminEndpoint).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.slotString.s172(),
				{
					port: 53844,
					accepted_authorities:
						fixtureCatalog.authority.existingAuthoritiesPort443(),
					mihomo_smux: {
						enabled: false,
						max_connections: 8,
						min_streams: 6,
						only_tcp: false,
					},
				},
			);
		});
	});

	it("does not render or submit SMux settings to a legacy endpoint API", async () => {
		vi.mocked(patchAdminEndpoint).mockResolvedValue({
			endpoint_id: fixtureCatalog.slotString.s172(),
			node_id: fixtureCatalog.slotString.s32(),
			tag: fixtureCatalog.slotString.s121(),
			kind: "vless_reality_vision_tcp",
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
				fixtureCatalog.slotString.s172(),
				{
					port: 53844,
					accepted_authorities:
						fixtureCatalog.authority.existingAuthoritiesPort443(),
				},
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
						node_id: fixtureCatalog.slotString.s124(),
						node_name: fixtureCatalog.slotString.s125(),
						access_host: fixtureCatalog.slotString.s126(),
						api_base_url: fixtureCatalog.slotString.s127(),
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
				endpoint_id: fixtureCatalog.slotString.s172(),
				node_id: fixtureCatalog.slotString.s124(),
				tag: fixtureCatalog.slotString.s129(),
				kind: "vless_reality_vision_tcp",
				port: 53844,
				meta: {
					reality: {
						dest: fixtureCatalog.address.loopback39043(),
						server_names: fixtureCatalog.slotList.l7(),
						server_names_source: "manual",
						fingerprint: "chrome",
					},
					managed_default: true,
				},
			});
			vi.mocked(patchAdminEndpoint).mockResolvedValue({
				endpoint_id: fixtureCatalog.slotString.s172(),
				node_id: fixtureCatalog.slotString.s124(),
				tag: fixtureCatalog.slotString.s129(),
				kind: "vless_reality_vision_tcp",
				port: 53844,
				meta: {
					reality: {
						dest: fixtureCatalog.address.loopback39043(),
						server_names: fixtureCatalog.slotList.l7(),
						server_names_source: "manual",
						fingerprint: "chrome",
					},
					managed_default: true,
					canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
					accepted_authorities: fixtureCatalog.slotList.l15(),
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
					fixtureCatalog.slotString.s172(),
					{
						port: 53844,
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
					node_id: fixtureCatalog.slotString.s32(),
					node_name: fixtureCatalog.slotString.s33(),
					access_host: fixtureCatalog.string.none(),
					api_base_url: fixtureCatalog.slotString.s123(),
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
