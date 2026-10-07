import { QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import { vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

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

const {
	mockReadAdminToken,
	mockUserId,
	mockFetchAdminUser,
	mockFetchAdminUserMihomoProfile,
	mockPutAdminUserMihomoProfile,
	mockPatchAdminUser,
	mockDeleteAdminUser,
	mockResetAdminUserToken,
	mockResetAdminUserCredentials,
} = vi.hoisted(() => ({
	mockReadAdminToken: vi.fn(() => "admintoken"),
	mockUserId: vi.fn(),
	mockFetchAdminUser: vi.fn(),
	mockFetchAdminUserMihomoProfile: vi.fn(),
	mockPutAdminUserMihomoProfile: vi.fn(),
	mockPatchAdminUser: vi.fn(),
	mockDeleteAdminUser: vi.fn(),
	mockResetAdminUserToken: vi.fn(),
	mockResetAdminUserCredentials: vi.fn(),
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
		useNavigate: () => vi.fn(),
		useParams: () => ({ userId: mockUserId() }),
	};
});

vi.mock("../api/adminUsers", async (importOriginal) => ({
	...(await importOriginal<typeof import("../api/adminUsers")>()),
	fetchAdminUser: mockFetchAdminUser,
	fetchAdminUserMihomoProfile: mockFetchAdminUserMihomoProfile,
	putAdminUserMihomoProfile: mockPutAdminUserMihomoProfile,
	patchAdminUser: mockPatchAdminUser,
	deleteAdminUser: mockDeleteAdminUser,
	resetAdminUserToken: mockResetAdminUserToken,
	resetAdminUserCredentials: mockResetAdminUserCredentials,
}));
vi.mock("../api/adminNodes");
vi.mock("../api/adminEndpoints");
vi.mock("../api/adminIpUsage");
vi.mock("../api/adminUserAccess");
vi.mock("../api/adminUserNodeQuotas");
vi.mock("../api/adminUserNodeQuotaStatus");
vi.mock("../api/subscription", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../api/subscription")>();
	return { ...actual, fetchSubscription: vi.fn() };
});

vi.mock("../components/auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../components/auth")>();
	return { ...actual, readAdminToken: mockReadAdminToken };
});

export {
	mockFetchAdminUserMihomoProfile,
	mockPutAdminUserMihomoProfile,
	mockReadAdminToken,
	mockUserId,
};

export function renderPage(queryClient = createQueryClient()) {
	if (typeof navigator !== "undefined" && !navigator.locks) {
		Object.defineProperty(navigator, "locks", {
			configurable: true,
			value: {
				request: async (_name: string, operation: () => Promise<unknown>) =>
					operation(),
			},
		});
	}
	const view = render(
		<QueryClientProvider client={queryClient}>
			<UiPrefsProvider>
				<ToastProvider>
					<UserDetailsPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
	return { ...view, queryClient };
}

export function setupMocks(args?: {
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
		user_id: fixtureCatalog.identifier.userPrimary(),
		display_name: "Ivan",
		subscription_token: fixtureCatalog.token.fixture254(),
		credential_epoch: 0,
		priority_tier: "p2",
		quota_reset: fixtureCatalog.quota.reset(),
	});

	vi.mocked(fetchAdminNodes).mockResolvedValue({
		items: [
			{
				node_id: fixtureCatalog.nodeId.fixture134(),
				node_name: fixtureCatalog.nodeName.fixture135(),
				api_base_url: fixtureCatalog.service.fixture136(),
				access_host: fixtureCatalog.host.fixture137(),
				quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
				quota_reset: fixtureCatalog.quota.resetNode(),
			},
		],
	});

	vi.mocked(fetchAdminEndpoints).mockResolvedValue({
		items: [
			{
				endpoint_id: fixtureCatalog.endpointId.fixture138(),
				node_id: fixtureCatalog.nodeId.fixture134(),
				tag: fixtureCatalog.endpointTag.fixture139(),
				kind: fixtureCatalog.endpoint.vlessKind(),
				port: fixtureCatalog.endpoint.port443(),
				meta: {},
			},
			{
				endpoint_id: fixtureCatalog.endpointId.fixture140(),
				node_id: fixtureCatalog.nodeId.fixture134(),
				tag: fixtureCatalog.endpointTag.fixture141(),
				kind: fixtureCatalog.endpoint.ssKind(),
				port: fixtureCatalog.endpoint.port8443(),
				meta: {},
			},
			{
				endpoint_id: fixtureCatalog.endpointId.fixture255(),
				node_id: fixtureCatalog.nodeId.fixture134(),
				tag: fixtureCatalog.endpointTag.fixture256(),
				kind: fixtureCatalog.endpoint.ssKind(),
				port: fixtureCatalog.endpoint.port9443(),
				meta: {},
			},
		],
	});

	vi.mocked(fetchAdminUserAccess).mockResolvedValue({
		items: args?.access ?? [],
		auto_assign_endpoint_kinds: [],
	});

	vi.mocked(fetchAdminUserNodeQuotas).mockResolvedValue({
		items: [
			{
				user_id: fixtureCatalog.identifier.userPrimary(),
				node_id: fixtureCatalog.nodeId.fixture134(),
				quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
				quota_reset_source: "user",
			},
		],
	});

	vi.mocked(fetchAdminUserNodeQuotaStatus).mockResolvedValue({
		partial: false,
		unreachable_nodes: [],
		items: [
			{
				user_id: fixtureCatalog.identifier.userPrimary(),
				node_id: fixtureCatalog.nodeId.fixture134(),
				quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
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
					user_id: fixtureCatalog.identifier.userPrimary(),
					display_name: "Ivan",
				},
				window,
				partial: false,
				unreachable_nodes: [],
				warnings: [],
				groups: [
					{
						node: {
							node_id: fixtureCatalog.nodeId.fixture134(),
							node_name: fixtureCatalog.nodeName.fixture135(),
							api_base_url: fixtureCatalog.service.fixture136(),
							access_host: fixtureCatalog.host.fixture137(),
							quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
							quota_reset: fixtureCatalog.quota.resetNode(),
						},
						geo_source: "country_is",
						window_start: fixtureCatalog.timestamp.t20260308T000000(),
						window_end: fixtureCatalog.timestamp.t20260308T000200(),
						warnings: [],
						unique_ip_series: [
							{
								minute: fixtureCatalog.timestamp.t20260308T000000(),
								count: 1,
							},
							{
								minute: fixtureCatalog.timestamp.t20260308T000100(),
								count: 2,
							},
						],
						timeline: [
							{
								lane_key: "edge-tokyo|203.0.113.7",
								endpoint_id: fixtureCatalog.endpointId.fixture40(),
								endpoint_tag: fixtureCatalog.endpointTag.fixture41(),
								ip: fixtureCatalog.address.documentation192_0_2_30(),
								minutes: 2,
								segments: [
									{
										start_minute: fixtureCatalog.timestamp.t20260308T000000(),
										end_minute: fixtureCatalog.timestamp.t20260308T000100(),
									},
								],
							},
						],
						ips: [
							{
								ip: fixtureCatalog.address.documentation192_0_2_30(),
								minutes: 2,
								endpoint_tags: [fixtureCatalog.endpointTag.fixture41()],
								region: "Japan / Tokyo",
								operator: "ExampleNet",
								last_seen_at: fixtureCatalog.timestamp.t20260308T000100(),
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
		auto_assign_endpoint_kinds: [],
	});
	vi.mocked(putAdminUserNodeQuota).mockResolvedValue({
		user_id: fixtureCatalog.identifier.userPrimary(),
		node_id: fixtureCatalog.nodeId.fixture134(),
		quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
		quota_reset_source: "user",
	});
	vi.mocked(patchAdminUser).mockResolvedValue({
		user_id: fixtureCatalog.identifier.userPrimary(),
		display_name: "Ivan",
		subscription_token: fixtureCatalog.token.fixture254(),
		credential_epoch: 0,
		priority_tier: "p2",
		quota_reset: fixtureCatalog.quota.reset(),
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
		subscription_token: fixtureCatalog.token.fixture257(),
	});
	vi.mocked(resetAdminUserCredentials).mockResolvedValue({
		user_id: fixtureCatalog.identifier.userPrimary(),
		credential_epoch: 1,
	});
	vi.mocked(fetchSubscription).mockResolvedValue(
		"vless://example-host?encryption=none",
	);
}
