import type { Meta, StoryObj } from "@storybook/react";

const userIdA = "01JQUSER000000000000000000";
const userIdB = "01JQUSER000000000000000001";

const meta = {
	title: "Pages/QuotaPolicyPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/quota-policy",
		},
		mockApi: {
			data: {
				nodes: [
					{
						node_id: "node-tokyo-a",
						node_name: "Tokyo-A",
						access_host: "tokyo-a.example.invalid",
						api_base_url: "https://tokyo-a.example.invalid",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
					{
						node_id: "node-frankfurt-b",
						node_name: "Frankfurt-B",
						access_host: "frankfurt-b.example.invalid",
						api_base_url: "https://frankfurt-b.example.invalid",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
					{
						node_id: "node-sydney-c",
						node_name: "Sydney-C",
						access_host: "sydney-c.example.invalid",
						api_base_url: "https://sydney-c.example.invalid",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
				],
				endpoints: [
					{
						endpoint_id: "ep-tokyo-a-vless",
						node_id: "node-tokyo-a",
						tag: "tokyo-a-vless",
						kind: "vless_reality_vision_tcp",
						port: 443,
						meta: {
							reality: {
								dest: "example.com:443",
								server_names: ["example.com"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
						},
					},
					{
						endpoint_id: "ep-tokyo-a-ss",
						node_id: "node-tokyo-a",
						tag: "tokyo-a-ss",
						kind: "ss2022_2022_blake3_aes_128_gcm",
						port: 8443,
						meta: {
							method: "2022-blake3-aes-128-gcm",
						},
					},
					{
						endpoint_id: "ep-frankfurt-b-vless",
						node_id: "node-frankfurt-b",
						tag: "frankfurt-b-vless",
						kind: "vless_reality_vision_tcp",
						port: 444,
						meta: {
							reality: {
								dest: "example.org:443",
								server_names: ["example.org"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
						},
					},
					{
						endpoint_id: "ep-sydney-c-vless",
						node_id: "node-sydney-c",
						tag: "sydney-c-vless",
						kind: "vless_reality_vision_tcp",
						port: 445,
						meta: {
							reality: {
								dest: "example.net:443",
								server_names: ["example.net"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
						},
					},
				],
				users: [
					{
						user_id: userIdA,
						display_name: "Ivan",
						subscription_token: `sub_${userIdA}`,
						priority_tier: "p1",
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: 480,
						},
					},
					{
						user_id: userIdB,
						display_name: "Koha",
						subscription_token: `sub_${userIdB}`,
						priority_tier: "p2",
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: 480,
						},
					},
				],
				userAccessByUser: {
					[userIdA]: [
						{
							membership: {
								user_id: userIdA,
								node_id: "node-tokyo-a",
								endpoint_id: "ep-tokyo-a-vless",
							},
							grant: {
								grant_id: "grant-demo-a1",
								enabled: true,
								quota_limit_bytes: 1,
								note: null,
								credentials: {
									vless: {
										uuid: "00000000-0000-0000-0000-000000000001",
										email: "grant:demo-1",
									},
								},
							},
						},
						{
							membership: {
								user_id: userIdA,
								node_id: "node-frankfurt-b",
								endpoint_id: "ep-frankfurt-b-vless",
							},
							grant: {
								grant_id: "grant-demo-a2",
								enabled: true,
								quota_limit_bytes: 1,
								note: null,
								credentials: {
									vless: {
										uuid: "00000000-0000-0000-0000-00000000000a",
										email: "grant:demo-b-1",
									},
								},
							},
						},
						{
							membership: {
								user_id: userIdA,
								node_id: "node-sydney-c",
								endpoint_id: "ep-sydney-c-vless",
							},
							grant: {
								grant_id: "grant-demo-a3",
								enabled: true,
								quota_limit_bytes: 1,
								note: null,
								credentials: {
									vless: {
										uuid: "00000000-0000-0000-0000-00000000000c",
										email: "grant:demo-c-1",
									},
								},
							},
						},
					],
					[userIdB]: [
						{
							membership: {
								user_id: userIdB,
								node_id: "node-tokyo-a",
								endpoint_id: "ep-tokyo-a-ss",
							},
							grant: {
								grant_id: "grant-demo-b1",
								enabled: true,
								quota_limit_bytes: 1,
								note: null,
								credentials: {
									ss2022: {
										method: "2022-blake3-aes-128-gcm",
										password: "secret",
									},
								},
							},
						},
						{
							membership: {
								user_id: userIdB,
								node_id: "node-frankfurt-b",
								endpoint_id: "ep-frankfurt-b-vless",
							},
							grant: {
								grant_id: "grant-demo-b2",
								enabled: true,
								quota_limit_bytes: 1,
								note: null,
								credentials: {
									vless: {
										uuid: "00000000-0000-0000-0000-00000000000b",
										email: "grant:demo-b-2",
									},
								},
							},
						},
						{
							membership: {
								user_id: userIdB,
								node_id: "node-sydney-c",
								endpoint_id: "ep-sydney-c-vless",
							},
							grant: {
								grant_id: "grant-demo-b3",
								enabled: true,
								quota_limit_bytes: 1,
								note: null,
								credentials: {
									ss2022: {
										method: "2022-blake3-aes-128-gcm",
										password: "secret",
									},
								},
							},
						},
					],
				},
				userNodeWeights: {
					[userIdA]: [
						{ node_id: "node-tokyo-a", weight: 6500 },
						{ node_id: "node-frankfurt-b", weight: 5000 },
						{ node_id: "node-sydney-c", weight: 2000 },
					],
					[userIdB]: [
						{ node_id: "node-tokyo-a", weight: 3500 },
						{ node_id: "node-frankfurt-b", weight: 5000 },
						{ node_id: "node-sydney-c", weight: 8000 },
					],
				},
				userGlobalWeights: {
					[userIdA]: 6500,
					[userIdB]: 3500,
				},
				nodeWeightPolicies: {
					"node-tokyo-a": {
						node_id: "node-tokyo-a",
						inherit_global: false,
					},
					"node-frankfurt-b": {
						node_id: "node-frankfurt-b",
						inherit_global: false,
					},
					"node-sydney-c": {
						node_id: "node-sydney-c",
						inherit_global: false,
					},
				},
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

const baseMockData = (meta.parameters as { mockApi?: { data?: unknown } })
	.mockApi?.data as Record<string, unknown>;

const TEN_USERS = Array.from({ length: 10 }, (_, index) => {
	const userId = `01JQUSER000000000000000${String(index).padStart(2, "0")}`;
	return {
		user_id: userId,
		display_name: `User-${String(index + 1).padStart(2, "0")}`,
		subscription_token: `sub_${userId}`,
		priority_tier: (index % 3 === 0 ? "p1" : index % 3 === 1 ? "p2" : "p3") as
			| "p1"
			| "p2"
			| "p3",
		quota_reset: {
			policy: "monthly" as const,
			day_of_month: 1,
			tz_offset_minutes: 480,
		},
	};
});

const TEN_USER_GLOBAL_WEIGHTS = Object.fromEntries(
	TEN_USERS.map((user, index) => [user.user_id, 1000 - index * 70]),
);

const TEN_USER_NODE_WEIGHTS = Object.fromEntries(
	TEN_USERS.map((user, index) => [
		user.user_id,
		[
			{
				node_id: "node-tokyo-a",
				weight: 1000 - index * 70,
			},
		],
	]),
);

const TEN_USER_ACCESS_BY_USER = Object.fromEntries(
	TEN_USERS.map((user, index) => {
		const endpointId = index % 2 === 0 ? "ep-tokyo-a-vless" : "ep-tokyo-a-ss";
		const credentials =
			index % 2 === 0
				? {
						vless: {
							uuid: `00000000-0000-0000-0000-${String(index + 1).padStart(
								12,
								"0",
							)}`,
							email: `grant:ten-users-${index + 1}`,
						},
					}
				: {
						ss2022: {
							method: "2022-blake3-aes-128-gcm",
							password: `secret-${index + 1}`,
						},
					};
		return [
			user.user_id,
			[
				{
					membership: {
						user_id: user.user_id,
						node_id: "node-tokyo-a",
						endpoint_id: endpointId,
					},
					grant: {
						grant_id: `grant-ten-${index + 1}`,
						enabled: true,
						quota_limit_bytes: 1,
						note: null,
						credentials,
					},
				},
			],
		];
	}),
);

export const TenUsers: Story = {
	parameters: {
		mockApi: {
			data: {
				...baseMockData,
				users: TEN_USERS,
				userAccessByUser: TEN_USER_ACCESS_BY_USER,
				userGlobalWeights: TEN_USER_GLOBAL_WEIGHTS,
				userNodeWeights: TEN_USER_NODE_WEIGHTS,
			},
		},
	},
};
