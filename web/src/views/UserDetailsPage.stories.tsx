import type { Meta, StoryObj } from "@storybook/react";

const meta = {
	title: "Pages/UserDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/users/user-1",
		},
		mockApi: {
			data: {
				grantGroups: [
					{
						group: { group_name: "managed-user-1" },
						members: [
							{
								user_id: "user-1",
								endpoint_id: "endpoint-1",
								enabled: true,
								quota_limit_bytes: 10 * 2 ** 30,
								note: null,
								credentials: {
									vless: {
										uuid: "22222222-2222-2222-2222-000000000001",
										email: "user1@example.com",
									},
								},
							},
							{
								user_id: "user-1",
								endpoint_id: "endpoint-2",
								enabled: true,
								quota_limit_bytes: 5 * 2 ** 30,
								note: null,
								credentials: {
									ss2022: {
										method: "2022-blake3-aes-128-gcm",
										password: "mock-password-2",
									},
								},
							},
						],
					},
				],
				nodeQuotas: [
					{
						user_id: "user-1",
						node_id: "node-1",
						quota_limit_bytes: 10 * 2 ** 30,
						quota_reset_source: "user",
					},
					{
						user_id: "user-1",
						node_id: "node-2",
						quota_limit_bytes: 5 * 2 ** 30,
						quota_reset_source: "node",
					},
				],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const User1: Story = {};

export const User2: Story = {
	parameters: {
		router: {
			initialEntry: "/users/user-2",
		},
	},
};
