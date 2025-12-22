import type { Meta, StoryObj } from "@storybook/react";

import { PageState } from "./PageState";

const meta: Meta<typeof PageState> = {
	title: "Components/PageState",
	component: PageState,
	args: {
		title: "Title",
		description: "Description",
	},
};

export default meta;

type Story = StoryObj<typeof PageState>;

export const Loading: Story = {
	args: { variant: "loading", title: "Loading" },
};

export const Empty: Story = {
	args: { variant: "empty", title: "No data" },
};

export const ErrorState: Story = {
	args: { variant: "error", title: "Something went wrong" },
};
