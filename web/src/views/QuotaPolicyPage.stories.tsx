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
						node_id: "node-1",
						node_name: "Node-1",
						access_host: "node-1.example.invalid",
						api_base_url: "https://node-1.example.invalid",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
					{
						node_id: "node-b",
						node_name: "Node B",
						access_host: "node-b.example.invalid",
						api_base_url: "https://node-b.example.invalid",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
					{
						node_id: "node-c",
						node_name: "节点三",
						access_host: "node-c.example.invalid",
						api_base_url: "https://node-c.example.invalid",
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
						endpoint_id: "ep-node-1-vless",
						node_id: "node-1",
						tag: "node-1-vless",
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
						endpoint_id: "ep-node-1-ss",
						node_id: "node-1",
						tag: "node-1-ss",
						kind: "ss2022_2022_blake3_aes_128_gcm",
						port: 8443,
						meta: {
							method: "2022-blake3-aes-128-gcm",
						},
					},
					{
						endpoint_id: "ep-node-b-vless",
						node_id: "node-b",
						tag: "node-b-vless",
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
						endpoint_id: "ep-node-c-vless",
						node_id: "node-c",
						tag: "node-c-vless",
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
				grantGroups: [
					{
						group: { group_name: "group-ratio-demo" },
						members: [
							{
								user_id: userIdA,
								endpoint_id: "ep-node-1-vless",
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
							{
								user_id: userIdB,
								endpoint_id: "ep-node-1-ss",
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
							{
								user_id: userIdA,
								endpoint_id: "ep-node-b-vless",
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
							{
								user_id: userIdB,
								endpoint_id: "ep-node-b-vless",
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
							{
								user_id: userIdA,
								endpoint_id: "ep-node-c-vless",
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
							{
								user_id: userIdB,
								endpoint_id: "ep-node-c-vless",
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
						],
					},
				],
				userNodeWeights: {
					[userIdA]: [
						{ node_id: "node-1", weight: 6500 },
						{ node_id: "node-b", weight: 5000 },
						{ node_id: "node-c", weight: 2000 },
					],
					[userIdB]: [
						{ node_id: "node-1", weight: 3500 },
						{ node_id: "node-b", weight: 5000 },
						{ node_id: "node-c", weight: 8000 },
					],
				},
				userGlobalWeights: {
					[userIdA]: 6500,
					[userIdB]: 3500,
				},
				nodeWeightPolicies: {
					"node-1": {
						node_id: "node-1",
						inherit_global: false,
					},
					"node-b": {
						node_id: "node-b",
						inherit_global: false,
					},
					"node-c": {
						node_id: "node-c",
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
