import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

import type { AlertsResponse } from "../api/adminAlerts";
import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminGrantGroupDetail } from "../api/adminGrantGroups";
import type { AdminGrant } from "../api/adminGrants";
import type { AdminNode } from "../api/adminNodes";
import type { AdminUserNodeQuota } from "../api/adminUserNodeQuotas";
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
		access_host: "tokyo.example.com",
		api_base_url: "https://n1:62416",
	},
	{
		node_id: "n2",
		node_name: "osaka-1",
		access_host: "osaka.example.com",
		api_base_url: "https://n2:62416",
	},
	{
		node_id: "n3",
		node_name: "nagoya-1",
		access_host: "nagoya.example.com",
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

const DESIGN_GRANT_GROUPS: AdminGrantGroupDetail[] = [
	{
		group: { group_name: "group-20260119-demo" },
		members: [
			{
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
			{
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
	},
];

const DESIGN_NODE_QUOTAS: AdminUserNodeQuota[] = [
	{
		user_id: "u_01HUSERAAAAAA",
		node_id: "n1",
		quota_limit_bytes: 10 * 2 ** 30,
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
		grantGroups: DESIGN_GRANT_GROUPS,
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
export const Grants: Story = pageStory({ path: "/grants" });
export const GrantNew: Story = pageStory({ path: "/grants/new" });
export const GrantNewMultiSelect: Story = {
	...pageStory({ path: "/grants/new" }),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.findByText("Create grant group")).resolves.toBeTruthy();
		await canvas.findByText("Selected 0 / 2");

		const toggleAll = await canvas.findByRole("checkbox", {
			name: "Toggle all nodes and protocols",
		});
		await userEvent.click(toggleAll);
		await canvas.findByText("Selected 2 / 2");

		await expect(
			await canvas.findByRole("button", { name: "Create group (2 members)" }),
		).toBeEnabled();
	},
};

export const GrantNewConflict: Story = {
	...pageStory({ path: "/grants/new" }),
	parameters: {
		router: { initialEntry: "/grants/new" },
		mockApi: {
			...DESIGN_MOCK_API,
			delayGrantGroupCreateMs: 200,
			failGrantGroupCreate: true,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await canvas.findByText("Selected 0 / 2");
		const toggleAll = await canvas.findByRole("checkbox", {
			name: "Toggle all nodes and protocols",
		});
		await userEvent.click(toggleAll);
		await canvas.findByText("Selected 2 / 2");

		const button = await canvas.findByRole("button", {
			name: "Create group (2 members)",
		});

		await userEvent.click(button);
		await expect(
			await canvas.findByText("409 conflict: group_name already exists", {
				selector: "p",
			}),
		).toBeInTheDocument();
	},
};
export const GrantDetails: Story = pageStory({
	path: "/grants/g_01HGRANTAAAAAA",
});
export const GrantGroupDetails: Story = pageStory({
	path: "/grant-groups/group-20260119-demo",
});
export const ServiceConfig: Story = pageStory({ path: "/service-config" });
export const ServiceConfigError: Story = pageStory({
	path: "/service-config",
	failAdminConfig: true,
});
