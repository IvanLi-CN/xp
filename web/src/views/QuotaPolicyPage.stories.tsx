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
						node_id: "node-tokyo",
						node_name: "tokyo-1",
						access_host: "tokyo-1.example.invalid",
						api_base_url: "https://tokyo-1.example.invalid",
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
						endpoint_id: "ep-tokyo-1",
						node_id: "node-tokyo",
						tag: "tokyo-vless",
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
						endpoint_id: "ep-tokyo-2",
						node_id: "node-tokyo",
						tag: "tokyo-ss",
						kind: "ss2022_2022_blake3_aes_128_gcm",
						port: 8443,
						meta: {
							method: "2022-blake3-aes-128-gcm",
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
								endpoint_id: "ep-tokyo-1",
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
								endpoint_id: "ep-tokyo-2",
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
					[userIdA]: [{ node_id: "node-tokyo", weight: 6500 }],
					[userIdB]: [{ node_id: "node-tokyo", weight: 3500 }],
				},
				userGlobalWeights: {
					[userIdA]: 6500,
					[userIdB]: 3500,
				},
				nodeWeightPolicies: {
					"node-tokyo": {
						node_id: "node-tokyo",
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
