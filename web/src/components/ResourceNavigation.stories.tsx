import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, userEvent, within } from "@storybook/test";

import {
	ResourceNavigation,
	type ResourceNavigationGroup,
} from "./ResourceNavigation";

const users = Array.from({ length: 12 }, (_, index) => {
	const number = String(index + 1).padStart(2, "0");
	return {
		id: `user-${number}`,
		label: `User ${number}`,
		href: `/users/user-${number}`,
		ariaLabel: `User ${number} (user-${number})`,
	};
});

const groups: ResourceNavigationGroup[] = [
	{
		title: "Nav",
		items: [
			{
				id: "dashboard",
				label: "Dashboard",
				href: "/",
				icon: "tabler:layout-dashboard",
			},
			{
				id: "users",
				label: "Users",
				href: "/users",
				icon: "tabler:users",
				children: users,
			},
		],
	},
];

const meta: Meta<typeof ResourceNavigation> = {
	title: "Components/ResourceNavigation",
	component: ResourceNavigation,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "padded",
	},
	args: {
		ariaLabel: "Primary navigation",
		groups,
		pathname: "/users",
		onNavigate: fn(),
		onResourceRequested: fn(),
	},
};

export default meta;

type Story = StoryObj<typeof ResourceNavigation>;

export const IndexCollapsed: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.queryByText("User 01")).toBeNull();
		await userEvent.click(canvas.getByRole("button", { name: "Expand Users" }));
		await expect(canvas.getByText("User 12")).toBeInTheDocument();
		const resourceList = canvas.getByTestId("resource-list-users");
		await expect(resourceList).toHaveClass("h-[20rem]");
		await expect(
			resourceList.querySelector("[data-radix-scroll-area-viewport]"),
		).toBeInTheDocument();
	},
};

export const ActiveObject: Story = {
	args: {
		pathname: "/users/user-12",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("button", { name: "Collapse Users" }),
		).toHaveAttribute("aria-expanded", "true");
		await expect(
			canvas.getByRole("link", { name: "User 12 (user-12)" }),
		).toHaveClass("bg-primary/10");
	},
};
