import type { Meta, StoryObj } from "@storybook/react";

const meta = {
	title: "Pages/UserDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/users/user-1",
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
