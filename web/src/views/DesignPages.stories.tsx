import type { Meta, StoryObj } from "@storybook/react";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type { AlertsResponse } from "../api/adminAlerts";
import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";
import type { AdminUserAccessItem } from "../api/adminUserAccess";
import type { AdminUserNodeQuota } from "../api/adminUserNodeQuotas";
import type { AdminUser } from "../api/adminUsers";
import type { NodeQuotaReset, UserQuotaReset } from "../api/quotaReset";

function Empty() {
	return <></>;
}

const meta: Meta<typeof Empty> = {
	title: "Design/Pages",
	component: Empty,
	// Keep this page gallery available for visual review, but exclude it from
	// Storybook's interaction test runner to avoid CI timeouts on full-page demos.
	tags: ["!test"],
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
			type: "quota_banned_membership",
			membership_key: "u_01HUSERAAAAAA::ep_01HENDPTAAAAAA",
			user_id: fixtureCatalog.identifier.userPrimary(),
			endpoint_id: fixtureCatalog.endpointId.fixture240(),
			owner_node_id: fixtureCatalog.nodeId.fixture241(),
			quota_banned: true,
			quota_banned_at: "2026-03-01T00:00:00Z",
			message: "Quota enforced on owner node (membership is blocked).",
			action_hint: "Wait for rollover/unban or adjust quota policy.",
		},
	],
};

const DESIGN_NODES: AdminNode[] = [
	{
		node_id: fixtureCatalog.nodeId.fixture241(),
		node_name: fixtureCatalog.nodeName.fixture33(),
		access_host: fixtureCatalog.host.fixture137(),
		api_base_url: fixtureCatalog.service.fixture242(),
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		} satisfies NodeQuotaReset,
	},
	{
		node_id: fixtureCatalog.nodeId.fixture243(),
		node_name: fixtureCatalog.nodeName.fixture37(),
		access_host: fixtureCatalog.host.fixture244(),
		api_base_url: fixtureCatalog.service.fixture245(),
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 15,
			tz_offset_minutes: null,
		} satisfies NodeQuotaReset,
	},
	{
		node_id: fixtureCatalog.nodeId.fixture246(),
		node_name: fixtureCatalog.nodeName.fixture247(),
		access_host: fixtureCatalog.host.fixture248(),
		api_base_url: fixtureCatalog.service.fixture249(),
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
		endpoint_id: fixtureCatalog.endpointId.fixture240(),
		node_id: fixtureCatalog.nodeId.fixture241(),
		tag: fixtureCatalog.endpointTag.fixture139(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: 443,
		meta: {
			reality: {
				dest: fixtureCatalog.address.loopbackPort39002(),
				server_names: fixtureCatalog.hostList.edge20(),
				server_names_source: "manual",
				fingerprint: "chrome",
			},
			canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
			accepted_authorities: fixtureCatalog.hostList.edge21(),
			managed_default: true,
		},
		short_ids: ["2a3b4c", "5d6e7f"],
		active_short_id: "2a3b4c",
	},
	{
		endpoint_id: fixtureCatalog.endpointId.fixture250(),
		node_id: fixtureCatalog.nodeId.fixture243(),
		tag: fixtureCatalog.endpointTag.fixture251(),
		kind: fixtureCatalog.endpoint.ssKind(),
		port: 8443,
		meta: {
			method: "2022-blake3-aes-128-gcm",
		},
		short_ids: ["aa11bb"],
		active_short_id: "aa11bb",
	},
];

const DESIGN_USERS: AdminUser[] = [
	{
		user_id: fixtureCatalog.identifier.userPrimary(),
		display_name: "Customer A",
		subscription_token: fixtureCatalog.token.fixture252(),
		credential_epoch: 0,
		priority_tier: "p3",
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: 480,
		} satisfies UserQuotaReset,
	},
	{
		user_id: fixtureCatalog.identifier.userSecondary(),
		display_name: "Customer B",
		subscription_token: fixtureCatalog.token.fixture253(),
		credential_epoch: 0,
		priority_tier: "p3",
		quota_reset: {
			policy: "monthly",
			day_of_month: 15,
			tz_offset_minutes: 480,
		} satisfies UserQuotaReset,
	},
];

const DESIGN_USER_ACCESS: Record<string, AdminUserAccessItem[]> = {
	[fixtureCatalog.identifier.userPrimary()]: [
		{
			user_id: fixtureCatalog.identifier.userPrimary(),
			endpoint_id: fixtureCatalog.endpointId.fixture240(),
			node_id: fixtureCatalog.nodeId.fixture241(),
		},
	],
	[fixtureCatalog.identifier.userSecondary()]: [
		{
			user_id: fixtureCatalog.identifier.userSecondary(),
			endpoint_id: fixtureCatalog.endpointId.fixture250(),
			node_id: fixtureCatalog.nodeId.fixture243(),
		},
	],
};

const DESIGN_NODE_QUOTAS: AdminUserNodeQuota[] = [
	{
		user_id: fixtureCatalog.identifier.userPrimary(),
		node_id: fixtureCatalog.nodeId.fixture241(),
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
			cluster_id: fixtureCatalog.cluster.fixture53(),
			node_id: fixtureCatalog.nodeId.fixture241(),
			role: "leader" as const,
			leader_api_base_url: fixtureCatalog.service.fixture242(),
			term: 42,
			xp_version: "0.0.0",
		},
		nodes: DESIGN_NODES,
		endpoints: DESIGN_ENDPOINTS,
		users: DESIGN_USERS,
		userAccessByUserId: DESIGN_USER_ACCESS,
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
						published_at: fixtureCatalog.timestamp.t20260131T000000(),
					},
					has_update: true,
					checked_at: fixtureCatalog.timestamp.t20260131T000000(),
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
export const Users: Story = pageStory({ path: "/users" });
export const UserNew: Story = pageStory({ path: "/users/new" });
export const UserDetails: Story = pageStory({ path: "/users/u_01HUSERAAAAAA" });
export const ServiceConfig: Story = pageStory({ path: "/service-config" });
export const Tools: Story = pageStory({ path: "/tools" });
export const ServiceConfigError: Story = pageStory({
	path: "/service-config",
	failAdminConfig: true,
});
