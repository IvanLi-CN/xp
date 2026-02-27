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
				grantGroups: [
					{
						group: { group_name: "group-ratio-demo" },
						members: [
							{
								user_id: userIdA,
								endpoint_id: "ep-tokyo-a-vless",
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
								endpoint_id: "ep-tokyo-a-ss",
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
								endpoint_id: "ep-frankfurt-b-vless",
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
								endpoint_id: "ep-frankfurt-b-vless",
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
								endpoint_id: "ep-sydney-c-vless",
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
								endpoint_id: "ep-sydney-c-vless",
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
