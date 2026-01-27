import type { Meta, StoryObj } from "@storybook/react";

const meta = {
	title: "Pages/LoginPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/login",
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const MissingToken: Story = {
	parameters: {
		mockApi: {
			adminToken: null,
		},
	},
};

export const WithToken: Story = {
	parameters: {
		mockApi: {
			adminToken: "storybook-admin-token",
		},
	},
};
