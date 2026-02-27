import type { Meta, StoryObj } from "@storybook/react";

import type { AlertsResponse } from "../api/adminAlerts";
import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";
import type { AdminRealityDomain } from "../api/adminRealityDomains";
import type { AdminUserGrant } from "../api/adminUserGrants";
import type { AdminUserNodeQuota } from "../api/adminUserNodeQuotas";
import type { AdminUser } from "../api/adminUsers";
import type { NodeQuotaReset, UserQuotaReset } from "../api/quotaReset";

function Empty() {
	return <></>;
}

const meta: Meta<typeof Empty> = {
	title: "Design/Pages",
	component: Empty,
	parameters: {
		layout: "fullscreen",
	},
};

export default meta;

type Story = StoryObj<typeof Empty>;

const DESIGN_ALERTS: AlertsResponse = {
	partial: false,
	unreachable_nodes: [],
	items: [
		{
			type: "quota_warning",
			grant_id: "g_01HGRANTAAAAAA",
			endpoint_id: "ep_01HENDPTAAAAAA",
			owner_node_id: "n1",
			desired_enabled: true,
			quota_banned: false,
			quota_banned_at: null,
			effective_enabled: true,
			message: "Usage is near the quota limit.",
			action_hint: "Consider raising the quota.",
		},
	],
};

const DESIGN_NODES: AdminNode[] = [
	{
		node_id: "n1",
		node_name: "tokyo-1",
		access_host: "tokyo.example.com",
		api_base_url: "https://n1:62416",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		} satisfies NodeQuotaReset,
	},
	{
		node_id: "n2",
		node_name: "osaka-1",
		access_host: "osaka.example.com",
		api_base_url: "https://n2:62416",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 15,
			tz_offset_minutes: null,
		} satisfies NodeQuotaReset,
	},
	{
		node_id: "n3",
		node_name: "nagoya-1",
		access_host: "nagoya.example.com",
		api_base_url: "https://n3:62416",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "unlimited",
			tz_offset_minutes: null,
		} satisfies NodeQuotaReset,
	},
];

const DESIGN_ENDPOINTS: Array<
	AdminEndpoint & { short_ids?: string[]; active_short_id?: string }
> = [
	{
		endpoint_id: "ep_01HENDPTAAAAAA",
		node_id: "n1",
		tag: "tokyo-vless",
		kind: "vless_reality_vision_tcp",
		port: 443,
		meta: {
			public_domain: "tokyo.example.com",
			reality: {
				dest: "www.example.com:443",
				server_names: ["example.com", "www.example.com"],
				fingerprint: "chrome",
			},
		},
		short_ids: ["2a3b4c", "5d6e7f"],
		active_short_id: "2a3b4c",
	},
	{
		endpoint_id: "ep_01HENDPTBBBBBB",
		node_id: "n2",
		tag: "osaka-ss",
		kind: "ss2022_2022_blake3_aes_128_gcm",
		port: 8443,
		meta: {
			method: "2022-blake3-aes-128-gcm",
		},
		short_ids: ["aa11bb"],
		active_short_id: "aa11bb",
	},
];

const DESIGN_REALITY_DOMAINS: AdminRealityDomain[] = [
	{
		domain_id: "seed_public_sn_files_1drv_com",
		server_name: "public.sn.files.1drv.com",
		disabled_node_ids: [],
	},
	{
		domain_id: "seed_public_bn_files_1drv_com",
		server_name: "public.bn.files.1drv.com",
		disabled_node_ids: ["n2"],
	},
];

const DESIGN_USERS: AdminUser[] = [
	{
		user_id: "u_01HUSERAAAAAA",
		display_name: "Customer A",
		subscription_token: "sub_9c1234d2",
		priority_tier: "p3",
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: 480,
		} satisfies UserQuotaReset,
	},
	{
		user_id: "u_01HUSERBBBBBB",
		display_name: "Customer B",
		subscription_token: "sub_af5678e9",
		priority_tier: "p3",
		quota_reset: {
			policy: "monthly",
			day_of_month: 15,
			tz_offset_minutes: 480,
		} satisfies UserQuotaReset,
	},
];

const DESIGN_USER_GRANTS: Record<string, AdminUserGrant[]> = {
	u_01HUSERAAAAAA: [
		{
			grant_id: "grant_01HGRANTAAAAAA",
			user_id: "u_01HUSERAAAAAA",
			endpoint_id: "ep_01HENDPTAAAAAA",
			enabled: true,
			quota_limit_bytes: 10_000_000,
			note: null,
			credentials: {
				vless: {
					uuid: "11111111-1111-1111-1111-111111111111",
					email: "customer-a@example.com",
				},
			},
		},
	],
	u_01HUSERBBBBBB: [
		{
			grant_id: "grant_01HGRANTBBBBBB",
			user_id: "u_01HUSERBBBBBB",
			endpoint_id: "ep_01HENDPTBBBBBB",
			enabled: true,
			quota_limit_bytes: 5_000_000,
			note: null,
			credentials: {
				ss2022: {
					method: "2022-blake3-aes-128-gcm",
					password: "mock-password",
				},
			},
		},
	],
};

const DESIGN_NODE_QUOTAS: AdminUserNodeQuota[] = [
	{
		user_id: "u_01HUSERAAAAAA",
		node_id: "n1",
		quota_limit_bytes: 10 * 2 ** 30,
		quota_reset_source: "user",
	},
];

const DESIGN_SUBSCRIPTIONS: Record<string, string> = {
	sub_9c1234d2: "# raw subscription for sub_9c1234d2\nnode: n1",
	sub_af5678e9: "# raw subscription for sub_af5678e9\nnode: n2",
};

const DESIGN_MOCK_API = {
	data: {
		health: { status: "ok" as const },
		clusterInfo: {
			cluster_id: "cluster-alpha",
			node_id: "n1",
			role: "leader" as const,
			leader_api_base_url: "https://n1:62416",
			term: 42,
			xp_version: "0.0.0",
		},
		nodes: DESIGN_NODES,
		endpoints: DESIGN_ENDPOINTS,
		realityDomains: DESIGN_REALITY_DOMAINS,
		users: DESIGN_USERS,
		userGrantsByUserId: DESIGN_USER_GRANTS,
		nodeQuotas: DESIGN_NODE_QUOTAS,
		alerts: DESIGN_ALERTS,
		subscriptions: DESIGN_SUBSCRIPTIONS,
	},
};

function pageStory(options: {
	path: string;
	adminToken?: string | null;
	failAdminConfig?: boolean;
}) {
	const { path, adminToken, failAdminConfig } = options;
	return {
		render: () => <></>,
		parameters: {
			router: { initialEntry: path },
			mockApi: {
				...DESIGN_MOCK_API,
				adminToken,
				failAdminConfig,
			},
		},
	} satisfies Story;
}

export const Login: Story = pageStory({ path: "/login", adminToken: null });
export const Dashboard: Story = pageStory({ path: "/" });
const DASHBOARD_BASE = pageStory({ path: "/" });
export const DashboardUpdateAvailable: Story = {
	...DASHBOARD_BASE,
	parameters: {
		...DASHBOARD_BASE.parameters,
		mockApi: {
			...(DASHBOARD_BASE.parameters?.mockApi ?? DESIGN_MOCK_API),
			data: {
				...DESIGN_MOCK_API.data,
				versionCheck: {
					current: { package: "0.1.0", release_tag: "v0.1.0" },
					latest: {
						release_tag: "v0.2.0",
						published_at: "2026-01-31T00:00:00Z",
					},
					has_update: true,
					checked_at: "2026-01-31T00:00:00Z",
					compare_reason: "semver",
					source: {
						kind: "github-releases",
						repo: "IvanLi-CN/xp",
						api_base: "https://api.github.com",
						channel: "stable",
					},
				},
			},
		},
	},
} satisfies Story;
export const DashboardUpdateFailed: Story = {
	...DASHBOARD_BASE,
	parameters: {
		...DASHBOARD_BASE.parameters,
		mockApi: {
			...(DASHBOARD_BASE.parameters?.mockApi ?? DESIGN_MOCK_API),
			failVersionCheck: true,
		},
	},
} satisfies Story;

export const Nodes: Story = pageStory({ path: "/nodes" });
export const NodeDetails: Story = pageStory({ path: "/nodes/n2" });
export const Endpoints: Story = pageStory({ path: "/endpoints" });
export const EndpointNew: Story = pageStory({ path: "/endpoints/new" });
export const EndpointDetails: Story = pageStory({
	path: "/endpoints/ep_01HENDPTAAAAAA",
});
export const RealityDomains: Story = pageStory({ path: "/reality-domains" });
export const Users: Story = pageStory({ path: "/users" });
export const UserNew: Story = pageStory({ path: "/users/new" });
export const UserDetails: Story = pageStory({ path: "/users/u_01HUSERAAAAAA" });
export const ServiceConfig: Story = pageStory({ path: "/service-config" });
export const ServiceConfigError: Story = pageStory({
	path: "/service-config",
	failAdminConfig: true,
});
