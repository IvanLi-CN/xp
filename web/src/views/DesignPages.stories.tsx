import type { Meta, StoryObj } from "@storybook/react";

import type { AlertsResponse } from "../api/adminAlerts";
import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminGrant } from "../api/adminGrants";
import type { AdminNode } from "../api/adminNodes";
import type { AdminUser } from "../api/adminUsers";

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
		{
			type: "quota_banned",
			grant_id: "g_01HGRANTBBBBBB",
			endpoint_id: "ep_01HENDPTBBBBBB",
			owner_node_id: "n2",
			desired_enabled: true,
			quota_banned: true,
			quota_banned_at: null,
			effective_enabled: true,
			message: "Grant is temporarily banned due to quota.",
			action_hint: "Review usage and adjust quota.",
		},
	],
};

const DESIGN_NODES: AdminNode[] = [
	{
		node_id: "n1",
		node_name: "tokyo-1",
		public_domain: "tokyo.example.com",
		api_base_url: "https://n1:62416",
	},
	{
		node_id: "n2",
		node_name: "osaka-1",
		public_domain: "osaka.example.com",
		api_base_url: "https://n2:62416",
	},
	{
		node_id: "n3",
		node_name: "nagoya-1",
		public_domain: "nagoya.example.com",
		api_base_url: "https://n3:62416",
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

const DESIGN_USERS: AdminUser[] = [
	{
		user_id: "u_01HUSERAAAAAA",
		display_name: "Customer A",
		subscription_token: "sub_9c1234d2",
		cycle_policy_default: "by_user",
		cycle_day_of_month_default: 1,
	},
	{
		user_id: "u_01HUSERBBBBBB",
		display_name: "Customer B",
		subscription_token: "sub_af5678e9",
		cycle_policy_default: "by_node",
		cycle_day_of_month_default: 15,
	},
];

const DESIGN_GRANTS: AdminGrant[] = [
	{
		grant_id: "g_01HGRANTAAAAAA",
		user_id: "u_01HUSERAAAAAA",
		endpoint_id: "ep_01HENDPTAAAAAA",
		enabled: true,
		quota_limit_bytes: 10_000_000,
		cycle_policy: "inherit_user",
		cycle_day_of_month: null,
		note: "Priority",
		credentials: {
			vless: {
				uuid: "11111111-1111-1111-1111-111111111111",
				email: "customer-a@example.com",
			},
		},
	},
	{
		grant_id: "g_01HGRANTBBBBBB",
		user_id: "u_01HUSERBBBBBB",
		endpoint_id: "ep_01HENDPTBBBBBB",
		enabled: true,
		quota_limit_bytes: 5_000_000,
		cycle_policy: "by_user",
		cycle_day_of_month: 15,
		note: null,
		credentials: {
			ss2022: {
				method: "2022-blake3-aes-128-gcm",
				password: "mock-password",
			},
		},
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
		},
		nodes: DESIGN_NODES,
		endpoints: DESIGN_ENDPOINTS,
		users: DESIGN_USERS,
		grants: DESIGN_GRANTS,
		alerts: DESIGN_ALERTS,
		subscriptions: DESIGN_SUBSCRIPTIONS,
	},
};

function pageStory(options: {
	path: string;
	theme: "light" | "dark";
	adminToken?: string | null;
}) {
	const { path, theme, adminToken } = options;
	return {
		render: () => <></>,
		parameters: {
			router: { initialEntry: path },
			ui: { theme },
			mockApi:
				adminToken === undefined
					? DESIGN_MOCK_API
					: { ...DESIGN_MOCK_API, adminToken },
		},
	} satisfies Story;
}

export const LoginLight: Story = pageStory({
	path: "/login",
	theme: "light",
	adminToken: null,
});

export const LoginDark: Story = pageStory({
	path: "/login",
	theme: "dark",
	adminToken: null,
});

export const DashboardLight: Story = pageStory({ path: "/", theme: "light" });
export const DashboardDark: Story = pageStory({ path: "/", theme: "dark" });

export const NodesLight: Story = pageStory({ path: "/nodes", theme: "light" });
export const NodesDark: Story = pageStory({ path: "/nodes", theme: "dark" });

export const NodeDetailsLight: Story = pageStory({
	path: "/nodes/n2",
	theme: "light",
});
export const NodeDetailsDark: Story = pageStory({
	path: "/nodes/n2",
	theme: "dark",
});

export const EndpointsLight: Story = pageStory({
	path: "/endpoints",
	theme: "light",
});
export const EndpointsDark: Story = pageStory({
	path: "/endpoints",
	theme: "dark",
});

export const EndpointNewLight: Story = pageStory({
	path: "/endpoints/new",
	theme: "light",
});
export const EndpointNewDark: Story = pageStory({
	path: "/endpoints/new",
	theme: "dark",
});

export const EndpointDetailsLight: Story = pageStory({
	path: "/endpoints/ep_01HENDPTAAAAAA",
	theme: "light",
});
export const EndpointDetailsDark: Story = pageStory({
	path: "/endpoints/ep_01HENDPTAAAAAA",
	theme: "dark",
});

export const UsersLight: Story = pageStory({ path: "/users", theme: "light" });
export const UsersDark: Story = pageStory({ path: "/users", theme: "dark" });

export const UserNewLight: Story = pageStory({
	path: "/users/new",
	theme: "light",
});
export const UserNewDark: Story = pageStory({
	path: "/users/new",
	theme: "dark",
});

export const UserDetailsLight: Story = pageStory({
	path: "/users/u_01HUSERAAAAAA",
	theme: "light",
});
export const UserDetailsDark: Story = pageStory({
	path: "/users/u_01HUSERAAAAAA",
	theme: "dark",
});

export const GrantsLight: Story = pageStory({
	path: "/grants",
	theme: "light",
});
export const GrantsDark: Story = pageStory({ path: "/grants", theme: "dark" });

export const GrantNewLight: Story = pageStory({
	path: "/grants/new",
	theme: "light",
});
export const GrantNewDark: Story = pageStory({
	path: "/grants/new",
	theme: "dark",
});

export const GrantDetailsLight: Story = pageStory({
	path: "/grants/g_01HGRANTAAAAAA",
	theme: "light",
});
export const GrantDetailsDark: Story = pageStory({
	path: "/grants/g_01HGRANTAAAAAA",
	theme: "dark",
});
