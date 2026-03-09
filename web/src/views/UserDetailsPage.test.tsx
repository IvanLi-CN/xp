import { QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import {
	type AdminUserIpUsageResponse,
	fetchAdminUserIpUsage,
} from "../api/adminIpUsage";
import { fetchAdminNodes } from "../api/adminNodes";
import {
	fetchAdminUserAccess,
	putAdminUserAccess,
} from "../api/adminUserAccess";
import { fetchAdminUserNodeQuotaStatus } from "../api/adminUserNodeQuotaStatus";
import {
	fetchAdminUserNodeQuotas,
	putAdminUserNodeQuota,
} from "../api/adminUserNodeQuotas";
import {
	deleteAdminUser,
	fetchAdminUser,
	fetchAdminUserMihomoProfile,
	patchAdminUser,
	putAdminUserMihomoProfile,
	resetAdminUserCredentials,
	resetAdminUserToken,
} from "../api/adminUsers";
import { fetchSubscription } from "../api/subscription";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { UserDetailsPage } from "./UserDetailsPage";

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
		useNavigate: () => vi.fn(),
		useParams: () => ({ userId: "u_01HUSERAAAAAA" }),
	};
});

vi.mock("../api/adminUsers");
vi.mock("../api/adminNodes");
vi.mock("../api/adminEndpoints");
vi.mock("../api/adminIpUsage");
vi.mock("../api/adminUserAccess");
vi.mock("../api/adminUserNodeQuotas");
vi.mock("../api/adminUserNodeQuotaStatus");
vi.mock("../api/adminIpUsage");
vi.mock("../api/subscription");

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
			<UiPrefsProvider>
				<ToastProvider>
					<UserDetailsPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

function setupMocks(args?: {
	access?: Array<{
		user_id: string;
		endpoint_id: string;
		node_id: string;
	}>;
	ipUsage?: AdminUserIpUsageResponse;
	userIpUsage?: AdminUserIpUsageResponse;
	mihomoProfile?: {
		mixin_yaml: string;
		extra_proxies_yaml: string;
		extra_proxy_providers_yaml: string;
	};
}) {
	vi.mocked(fetchAdminUser).mockResolvedValue({
		user_id: "u_01HUSERAAAAAA",
		display_name: "Ivan",
		subscription_token: "subtoken",
		credential_epoch: 0,
		priority_tier: "p2",
		quota_reset: { policy: "monthly", day_of_month: 1, tz_offset_minutes: 480 },
	});

	vi.mocked(fetchAdminNodes).mockResolvedValue({
		items: [
			{
				node_id: "node-tokyo",
				node_name: "Tokyo",
				api_base_url: "https://tokyo.example.com",
				access_host: "tokyo.example.com",
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
				endpoint_id: "ep-vless",
				node_id: "node-tokyo",
				tag: "tokyo-vless",
				kind: "vless_reality_vision_tcp",
				port: 443,
				meta: {},
			},
			{
				endpoint_id: "ep-ss",
				node_id: "node-tokyo",
				tag: "tokyo-ss",
				kind: "ss2022_2022_blake3_aes_128_gcm",
				port: 8443,
				meta: {},
			},
			{
				endpoint_id: "ep-ss-2",
				node_id: "node-tokyo",
				tag: "tokyo-ss-2",
				kind: "ss2022_2022_blake3_aes_128_gcm",
				port: 9443,
				meta: {},
			},
		],
	});

	vi.mocked(fetchAdminUserAccess).mockResolvedValue({
		items: args?.access ?? [],
	});

	vi.mocked(fetchAdminUserNodeQuotas).mockResolvedValue({
		items: [
			{
				user_id: "u_01HUSERAAAAAA",
				node_id: "node-tokyo",
				quota_limit_bytes: 0,
				quota_reset_source: "user",
			},
		],
	});

	vi.mocked(fetchAdminUserNodeQuotaStatus).mockResolvedValue({
		partial: false,
		unreachable_nodes: [],
		items: [
			{
				user_id: "u_01HUSERAAAAAA",
				node_id: "node-tokyo",
				quota_limit_bytes: 1024,
				used_bytes: 0,
				remaining_bytes: 1024,
				cycle_end_at: null,
				quota_reset_source: "user",
			},
		],
	});

	vi.mocked(fetchAdminUserIpUsage).mockImplementation(
		async (_token, _userId, window) =>
			args?.ipUsage ??
			args?.userIpUsage ?? {
				user: {
					user_id: "u_01HUSERAAAAAA",
					display_name: "Ivan",
				},
				window,
				partial: false,
				unreachable_nodes: [],
				warnings: [],
				groups: [
					{
						node: {
							node_id: "node-tokyo",
							node_name: "Tokyo",
							api_base_url: "https://tokyo.example.com",
							access_host: "tokyo.example.com",
							quota_limit_bytes: 0,
							quota_reset: {
								policy: "monthly",
								day_of_month: 1,
								tz_offset_minutes: null,
							},
						},
						window_start: "2026-03-08T00:00:00Z",
						window_end: "2026-03-08T00:02:00Z",
						warnings: [],
						unique_ip_series: [
							{ minute: "2026-03-08T00:00:00Z", count: 1 },
							{ minute: "2026-03-08T00:01:00Z", count: 2 },
						],
						timeline: [
							{
								lane_key: "edge-tokyo|203.0.113.7",
								endpoint_id: "endpoint-1",
								endpoint_tag: "edge-tokyo",
								ip: "203.0.113.7",
								minutes: 2,
								segments: [
									{
										start_minute: "2026-03-08T00:00:00Z",
										end_minute: "2026-03-08T00:01:00Z",
									},
								],
							},
						],
						ips: [
							{
								ip: "203.0.113.7",
								minutes: 2,
								endpoint_tags: ["edge-tokyo"],
								region: "Japan / Tokyo",
								operator: "ExampleNet",
								last_seen_at: "2026-03-08T00:01:00Z",
							},
						],
					},
				],
			},
	);

	vi.mocked(putAdminUserAccess).mockResolvedValue({
		created: 0,
		deleted: 0,
		items: [],
	});
	vi.mocked(putAdminUserNodeQuota).mockResolvedValue({
		user_id: "u_01HUSERAAAAAA",
		node_id: "node-tokyo",
		quota_limit_bytes: 0,
		quota_reset_source: "user",
	});
	vi.mocked(patchAdminUser).mockResolvedValue({
		user_id: "u_01HUSERAAAAAA",
		display_name: "Ivan",
		subscription_token: "subtoken",
		credential_epoch: 0,
		priority_tier: "p2",
		quota_reset: { policy: "monthly", day_of_month: 1, tz_offset_minutes: 480 },
	});
	vi.mocked(deleteAdminUser).mockResolvedValue(undefined);
	vi.mocked(fetchAdminUserMihomoProfile).mockResolvedValue(
		args?.mihomoProfile ?? {
			mixin_yaml: "",
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: "",
		},
	);
	vi.mocked(putAdminUserMihomoProfile).mockResolvedValue({
		mixin_yaml: "",
		extra_proxies_yaml: "",
		extra_proxy_providers_yaml: "",
	});
	vi.mocked(resetAdminUserToken).mockResolvedValue({
		subscription_token: "sub_new",
	});
	vi.mocked(resetAdminUserCredentials).mockResolvedValue({
		user_id: "u_01HUSERAAAAAA",
		credential_epoch: 1,
	});
	vi.mocked(fetchSubscription).mockResolvedValue(
		"vless://example-host?encryption=none",
	);
}

describe("<UserDetailsPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		mockReadAdminToken.mockReturnValue("admintoken");
	});

	afterEach(() => {
		cleanup();
	});

	it("initializes access matrix from existing user access", async () => {
		setupMocks({
			access: [
				{
					user_id: "u_01HUSERAAAAAA",
					endpoint_id: "ep-vless",
					node_id: "node-tokyo",
				},
			],
		});
		renderPage();

		await waitFor(() => {
			expect(fetchAdminUserAccess).toHaveBeenCalled();
		});

		const accessTab = await screenByRole("button", "Access");
		fireEvent.click(accessTab);

		const checkbox = await screenByLabel("Toggle Tokyo VLESS");
		expect((checkbox as HTMLInputElement).checked).toBe(true);
	});

	it("shows per-node remaining quota without node id text in access matrix", async () => {
		setupMocks();
		renderPage();

		const accessTab = await screenByRole("button", "Access");
		fireEvent.click(accessTab);

		expect(await screenByText("Remaining: 0 MiB")).toBeTruthy();
		expect(await queryByText("node-tokyo")).toBeNull();
	});

	it("applies selected endpoints via putAdminUserAccess", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		fireEvent.click(await screenByLabel("Toggle Tokyo VLESS"));
		fireEvent.click(await screenByRole("button", "Apply access"));

		await waitFor(() => {
			expect(putAdminUserAccess).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				{
					items: [
						{
							endpoint_id: "ep-vless",
						},
					],
				},
			);
		});
	});

	it("submits empty items when all access is cleared", async () => {
		setupMocks({
			access: [
				{
					user_id: "u_01HUSERAAAAAA",
					endpoint_id: "ep-vless",
					node_id: "node-tokyo",
				},
			],
		});
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		fireEvent.click(await screenByLabel("Toggle Tokyo VLESS"));
		fireEvent.click(await screenByRole("button", "Apply access"));

		await waitFor(() => {
			expect(putAdminUserAccess).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				{ items: [] },
			);
		});
	});

	it("shows single endpoint metadata when unchecked", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));

		expect(await screenByText("port 443")).toBeTruthy();
		expect(await queryByText("port ?")).toBeNull();
	});

	it("supports multi-select endpoint leaves in access tree", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		fireEvent.click(
			await screenByLabel(
				"Select endpoint tokyo-ss for node-tokyo ss2022_2022_blake3_aes_128_gcm",
			),
		);
		fireEvent.click(
			await screenByLabel(
				"Select endpoint tokyo-ss-2 for node-tokyo ss2022_2022_blake3_aes_128_gcm",
			),
		);
		fireEvent.click(await screenByRole("button", "Apply access"));

		await waitFor(() => {
			expect(putAdminUserAccess).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				{
					items: expect.arrayContaining([
						{ endpoint_id: "ep-ss" },
						{ endpoint_id: "ep-ss-2" },
					]),
				},
			);
		});
	});

	it("shows partial state in row and column toggles for tree leaf selection", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		fireEvent.click(
			await screenByLabel(
				"Select endpoint tokyo-ss for node-tokyo ss2022_2022_blake3_aes_128_gcm",
			),
		);

		const allToggle = (await screenByLabel("Toggle all")) as HTMLInputElement;
		const rowToggle = (await screenByLabel(
			"Toggle row Tokyo",
		)) as HTMLInputElement;
		const columnToggle = (await screenByLabel(
			"Toggle SS2022",
		)) as HTMLInputElement;

		expect(allToggle.checked).toBe(false);
		expect(allToggle.indeterminate).toBe(true);
		expect(rowToggle.checked).toBe(false);
		expect(rowToggle.indeterminate).toBe(true);
		expect(columnToggle.checked).toBe(false);
		expect(columnToggle.indeterminate).toBe(true);
	});

	it("does not render legacy outer checkbox for multi-endpoint cell", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));

		expect(await queryByLabel("Toggle Tokyo SS2022")).toBeNull();
		expect(
			await screenByLabel(
				"Toggle all endpoints for node-tokyo ss2022_2022_blake3_aes_128_gcm",
			),
		).toBeTruthy();
	});

	it("checks all endpoint leaves when tree parent is enabled", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		fireEvent.click(
			await screenByLabel(
				"Toggle all endpoints for node-tokyo ss2022_2022_blake3_aes_128_gcm",
			),
		);

		expect(
			(
				(await screenByLabel(
					"Select endpoint tokyo-ss for node-tokyo ss2022_2022_blake3_aes_128_gcm",
				)) as HTMLInputElement
			).checked,
		).toBe(true);
		expect(
			(
				(await screenByLabel(
					"Select endpoint tokyo-ss-2 for node-tokyo ss2022_2022_blake3_aes_128_gcm",
				)) as HTMLInputElement
			).checked,
		).toBe(true);

		fireEvent.click(await screenByRole("button", "Apply access"));

		await waitFor(() => {
			expect(putAdminUserAccess).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				{
					items: expect.arrayContaining([
						{ endpoint_id: "ep-ss" },
						{ endpoint_id: "ep-ss-2" },
					]),
				},
			);
		});
	});

	it("uses fixed table layout with in-flow tree panel", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));

		const table = document.body.querySelector("table");
		expect(table?.className).toContain("table-fixed");
		const cols = Array.from(table?.querySelectorAll("col") ?? []);
		expect(cols).toHaveLength(4);
		expect(cols[0]?.getAttribute("style") ?? "").toContain("2.5rem");
		expect(cols[1]?.getAttribute("style") ?? "").toContain("14rem");
		expect(cols[2]?.getAttribute("style") ?? "").toContain("16.5rem");

		const tree = await screenByRole(
			"tree",
			"Endpoint options for node-tokyo ss2022_2022_blake3_aes_128_gcm",
		);
		expect((tree.parentElement as HTMLElement | null)?.className).not.toContain(
			"absolute",
		);
	});

	it("keeps tree all-select usable while collapsed", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		fireEvent.click(
			await screenByLabel(
				"Toggle endpoint tree for node-tokyo ss2022_2022_blake3_aes_128_gcm",
			),
		);
		fireEvent.click(
			await screenByLabel(
				"Toggle all endpoints for node-tokyo ss2022_2022_blake3_aes_128_gcm",
			),
		);
		fireEvent.click(
			await screenByLabel(
				"Toggle endpoint tree for node-tokyo ss2022_2022_blake3_aes_128_gcm",
			),
		);

		expect(
			(
				(await screenByLabel(
					"Select endpoint tokyo-ss for node-tokyo ss2022_2022_blake3_aes_128_gcm",
				)) as HTMLInputElement
			).checked,
		).toBe(true);
		expect(
			(
				(await screenByLabel(
					"Select endpoint tokyo-ss-2 for node-tokyo ss2022_2022_blake3_aes_128_gcm",
				)) as HTMLInputElement
			).checked,
		).toBe(true);
	});

	it("saves edited user profile", async () => {
		setupMocks();
		renderPage();

		const input = await screenByLabel("Display name");
		fireEvent.change(input, { target: { value: "Ivan Updated" } });
		fireEvent.click(await screenByRole("button", "Save user"));

		await waitFor(() => {
			expect(patchAdminUser).toHaveBeenCalled();
		});
	});

	it("loads subscription preview from user tab", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("button", "Fetch"));

		await waitFor(() => {
			expect(fetchSubscription).toHaveBeenCalledWith("subtoken", "raw");
		});
		expect(await screenByText("Subscription preview")).toBeTruthy();
	});

	it("supports mihomo subscription format preview", async () => {
		setupMocks();
		renderPage();

		fireEvent.change(await screenByLabel("Subscription format"), {
			target: { value: "mihomo" },
		});
		fireEvent.click(await screenByRole("button", "Fetch"));

		await waitFor(() => {
			expect(fetchSubscription).toHaveBeenCalledWith("subtoken", "mihomo");
		});
	});

	it("saves mihomo profile from user tab", async () => {
		setupMocks();
		renderPage();

		fireEvent.change(await screenByLabel("mixin_yaml"), {
			target: { value: "port: 0\nproxy-groups: []\n" },
		});
		fireEvent.click(await screenByRole("button", "Save mihomo mixin"));

		await waitFor(() => {
			expect(putAdminUserMihomoProfile).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				{
					mixin_yaml: "port: 0\nproxy-groups: []\n",
					extra_proxies_yaml: "",
					extra_proxy_providers_yaml: "",
				},
			);
		});
	});

	it("normalizes legacy dynamic sections before saving mihomo profile", async () => {
		setupMocks({
			mihomoProfile: {
				mixin_yaml: `port: 0
proxy-providers:
  providerA:
    type: http
    path: ./provider-a-from-mixin.yaml
    url: https://example.com/sub-a-from-mixin
rules: []
`,
				extra_proxies_yaml: "",
				extra_proxy_providers_yaml: `providerA:
  type: http
  path: ./provider-a-from-extra.yaml
  url: https://example.com/sub-a-from-extra
`,
			},
		});
		renderPage();

		await waitFor(() => {
			expect(fetchAdminUserMihomoProfile).toHaveBeenCalled();
		});
		fireEvent.click(await screenByRole("button", "Save mihomo mixin"));

		await waitFor(() => {
			expect(putAdminUserMihomoProfile).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				{
					mixin_yaml: "port: 0\nrules: []\n",
					extra_proxies_yaml: "",
					extra_proxy_providers_yaml: `providerA:
  type: http
  path: ./provider-a-from-extra.yaml
  url: https://example.com/sub-a-from-extra
`,
				},
			);
		});
	});

	it("shows node quota loading error instead of defaulting to zero", async () => {
		setupMocks();
		vi.mocked(fetchAdminUserNodeQuotas).mockRejectedValueOnce(
			new Error("node quotas unavailable"),
		);
		renderPage();

		expect(await screenByText("Failed to load node quotas")).toBeTruthy();
		expect(await screenByText("node quotas unavailable")).toBeTruthy();
	});

	it("shows access data error with retry action", async () => {
		setupMocks();
		vi.mocked(fetchAdminEndpoints).mockRejectedValue(
			new Error("endpoint load failed"),
		);
		renderPage();

		fireEvent.click(await screenByRole("button", "Access"));
		expect(await screenByText("Failed to load access matrix")).toBeTruthy();
		const retryButton = await screenByRole("button", "Retry access data");
		fireEvent.click(retryButton);

		await waitFor(() => {
			expect(fetchAdminEndpoints).toHaveBeenCalledTimes(2);
		});
	});

	it("shows partial quota status warning with unreachable nodes", async () => {
		setupMocks();
		vi.mocked(fetchAdminUserNodeQuotaStatus).mockResolvedValueOnce({
			partial: true,
			unreachable_nodes: ["node-osaka"],
			items: [
				{
					user_id: "u_01HUSERAAAAAA",
					node_id: "node-tokyo",
					quota_limit_bytes: 1024,
					used_bytes: 0,
					remaining_bytes: 1024,
					cycle_end_at: null,
					quota_reset_source: "user",
				},
			],
		});
		renderPage();

		fireEvent.click(await screenByRole("button", "Quota status"));
		expect(await screenByText("Quota status is partial.")).toBeTruthy();
		expect(await screenByText("Unreachable nodes: node-osaka")).toBeTruthy();
	});

	it("renders usage details groups and refetches when the window changes", async () => {
		setupMocks({
			ipUsage: {
				user: {
					user_id: "u_01HUSERAAAAAA",
					display_name: "Ivan",
				},
				window: "24h",
				partial: false,
				unreachable_nodes: [],
				warnings: [],
				groups: [
					{
						node: {
							node_id: "node-tokyo",
							node_name: "Tokyo",
							api_base_url: "https://tokyo.example.com",
							access_host: "tokyo.example.com",
							quota_limit_bytes: 0,
							quota_reset: {
								policy: "monthly",
								day_of_month: 1,
								tz_offset_minutes: null,
							},
						},
						window_start: "2026-03-08T00:00:00Z",
						window_end: "2026-03-08T00:02:00Z",
						warnings: [],
						unique_ip_series: [{ minute: "2026-03-08T00:00:00Z", count: 1 }],
						timeline: [],
						ips: [
							{
								ip: "203.0.113.7",
								minutes: 1,
								endpoint_tags: ["edge-tokyo"],
								region: "Japan / Tokyo",
								operator: "ExampleNet",
								last_seen_at: "2026-03-08T00:00:00Z",
							},
						],
					},
					{
						node: {
							node_id: "node-osaka",
							node_name: "Osaka",
							api_base_url: "https://osaka.example.com",
							access_host: "osaka.example.com",
							quota_limit_bytes: 0,
							quota_reset: {
								policy: "monthly",
								day_of_month: 1,
								tz_offset_minutes: null,
							},
						},
						window_start: "2026-03-08T00:00:00Z",
						window_end: "2026-03-08T00:02:00Z",
						warnings: [],
						unique_ip_series: [{ minute: "2026-03-08T00:00:00Z", count: 1 }],
						timeline: [],
						ips: [
							{
								ip: "198.51.100.9",
								minutes: 1,
								endpoint_tags: ["edge-osaka"],
								region: "Japan / Osaka",
								operator: "CarrierNet",
								last_seen_at: "2026-03-08T00:00:00Z",
							},
						],
					},
				],
			},
		});
		renderPage();

		fireEvent.click(await screenByRole("button", "Usage details"));

		await waitFor(() => {
			expect(fetchAdminUserIpUsage).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				"24h",
				expect.any(AbortSignal),
			);
		});
		await waitFor(() => {
			expect(screen.getByText("Usage details · Tokyo")).toBeInTheDocument();
		});
		expect(screen.getByRole("tab", { name: "Tokyo" })).toHaveAttribute(
			"aria-selected",
			"true",
		);
		expect(screen.queryByText("Usage details · Osaka")).not.toBeInTheDocument();

		fireEvent.click(screen.getByRole("tab", { name: "Osaka" }));
		await waitFor(() => {
			expect(screen.getByText("Usage details · Osaka")).toBeInTheDocument();
		});
		expect(screen.getByRole("tab", { name: "Osaka" })).toHaveAttribute(
			"aria-selected",
			"true",
		);
		const windowButtons = screen.getAllByRole("button", { name: "7d" });
		fireEvent.click(windowButtons[0]);
		await waitFor(() => {
			expect(fetchAdminUserIpUsage).toHaveBeenCalledWith(
				"admintoken",
				"u_01HUSERAAAAAA",
				"7d",
				expect.any(AbortSignal),
			);
		});
	});

	it("shows unreachable-only empty state when all usage nodes fail", async () => {
		setupMocks({
			ipUsage: {
				user: {
					user_id: "u_01HUSERAAAAAA",
					display_name: "Ivan",
				},
				window: "24h",
				partial: true,
				unreachable_nodes: ["node-osaka"],
				warnings: [],
				groups: [],
			},
		});
		renderPage();

		fireEvent.click(await screenByRole("button", "Usage details"));
		await waitFor(() => {
			expect(
				screen.getByText("Usage details are partial."),
			).toBeInTheDocument();
		});
		expect(
			screen.getByText("Unreachable nodes: node-osaka"),
		).toBeInTheDocument();
		expect(screen.getByText("Usage details unavailable")).toBeInTheDocument();
		expect(
			screen.getByText(
				"All nodes for this user's inbound IP usage are currently unreachable.",
			),
		).toBeInTheDocument();
		expect(
			screen.queryByText(
				"This user has no active node memberships to aggregate inbound IP usage from.",
			),
		).not.toBeInTheDocument();
	});

	it("shows token-required state when admin token is missing", async () => {
		mockReadAdminToken.mockReturnValue("");
		setupMocks();
		renderPage();

		expect(await screenByText("Admin token required")).toBeTruthy();
		expect(fetchAdminUser).not.toHaveBeenCalled();
	});
});

async function screenByRole(role: string, name: string): Promise<HTMLElement> {
	const { findByRole } = await import("@testing-library/react");
	return findByRole(document.body, role as never, { name });
}

async function screenByLabel(label: string): Promise<HTMLElement> {
	const { findByLabelText } = await import("@testing-library/react");
	return findByLabelText(document.body, label);
}

async function screenByText(text: string): Promise<HTMLElement> {
	const { findByText } = await import("@testing-library/react");
	return findByText(document.body, text);
}

async function queryByLabel(label: string): Promise<HTMLElement | null> {
	const { queryByLabelText } = await import("@testing-library/react");
	return queryByLabelText(document.body, label);
}

async function queryByText(text: string): Promise<HTMLElement | null> {
	const { queryByText } = await import("@testing-library/react");
	return queryByText(document.body, text);
}
