import type { Meta, StoryObj } from "@storybook/react";
import {
	expect,
	fn,
	screen,
	userEvent,
	waitFor,
	within,
} from "@storybook/test";

import {
	ResourceNavigation,
	type ResourceNavigationGroup,
} from "./ResourceNavigation";

const LONG_ENDPOINT =
	"singapore-reality-edge-with-an-intentionally-long-descriptive-name";
const LONG_USER =
	"A user name long enough to verify the selected navigation capsule";

const users = Array.from({ length: 12 }, (_, index) => {
	const number = String(index + 1).padStart(2, "0");
	const label = index === 11 ? LONG_USER : `User ${number}`;
	return {
		id: `user-${number}`,
		label,
		href: `/users/user-${number}`,
		ariaLabel: `${label} (user-${number})`,
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
				id: "nodes",
				label: "Nodes",
				href: "/nodes",
				icon: "tabler:server",
				children: [
					{
						id: "node-tokyo-1",
						label: "tokyo-1",
						href: "/nodes/node-tokyo-1",
						ariaLabel: "Current hosting node tokyo-1 (node-tokyo-1)",
						leadingIcon: {
							name: "tabler:server-bolt",
							tone: "primary",
						},
					},
					{
						id: "node-osaka-1",
						label: "osaka-1",
						href: "/nodes/node-osaka-1",
						ariaLabel: "Node osaka-1 (node-osaka-1)",
						leadingIcon: {
							name: "tabler:server",
							tone: "muted",
						},
					},
				],
			},
			{
				id: "endpoints",
				label: "Endpoints",
				href: "/endpoints",
				icon: "tabler:plug",
				children: [
					{
						id: "endpoint-sgp",
						label: LONG_ENDPOINT,
						href: "/endpoints/endpoint-sgp",
						ariaLabel: `Endpoint ${LONG_ENDPOINT} (endpoint-sgp)`,
					},
				],
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
	tags: ["autodocs", "coverage-ui", "resource-navigation-polish"],
	parameters: {
		layout: "padded",
		docs: {
			description: {
				component: [
					"Resource navigation with bounded vertical lists, hosting-node identity,",
					"complete selected capsules, and motion-aware overflow labels.",
				].join(" "),
			},
		},
	},
	decorators: [
		(Story) => (
			<div className="w-[320px]">
				<Story />
			</div>
		),
	],
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
		await expect(canvas.getByText(LONG_USER)).toBeInTheDocument();
		const resourceList = canvas.getByTestId("resource-list-users");
		await expect(resourceList).toHaveClass("h-[20rem]");
		await expect(
			resourceList.querySelector("[data-radix-scroll-area-viewport]"),
		).toBeInTheDocument();
	},
};

export const ActiveObjectCapsule: Story = {
	args: {
		pathname: "/users/user-12",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const activeLink = canvas.getByRole("link", {
			name: `${LONG_USER} (user-12)`,
		});
		const resourceList = canvas.getByTestId("resource-list-users");
		const viewport = resourceList.querySelector<HTMLElement>(
			"[data-radix-scroll-area-viewport]",
		);
		if (!viewport) throw new Error("Resource viewport is missing.");

		await expect(activeLink).toHaveClass("rounded-full");
		await waitFor(() => {
			expect(viewport.scrollWidth).toBeLessThanOrEqual(viewport.clientWidth);
			expect(activeLink.getBoundingClientRect().right).toBeLessThanOrEqual(
				viewport.getBoundingClientRect().right + 0.5,
			);
		});
	},
};

export const HostingNodeIdentity: Story = {
	args: {
		pathname: "/nodes/node-tokyo-1",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("link", {
				name: "Current hosting node tokyo-1 (node-tokyo-1)",
			}),
		).toHaveAttribute("data-leading-icon-tone", "primary");
		await expect(
			canvas.getByRole("link", { name: "Node osaka-1 (node-osaka-1)" }),
		).toHaveAttribute("data-leading-icon-tone", "muted");
	},
};

export const LongNameReveal: Story = {
	args: {
		pathname: "/endpoints/endpoint-sgp",
		reducedMotionOverride: false,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const link = canvas.getByRole("link", {
			name: `Endpoint ${LONG_ENDPOINT} (endpoint-sgp)`,
		});
		const labelViewport = canvas.getByText(LONG_ENDPOINT).parentElement;
		if (!labelViewport) throw new Error("Label viewport is missing.");
		await waitFor(() =>
			expect(labelViewport).toHaveAttribute("data-overflowing", "true"),
		);
		await userEvent.hover(link);
		await waitFor(
			() =>
				expect(labelViewport).toHaveAttribute("data-reveal-phase", "forward"),
			{ timeout: 1_500 },
		);
		await expect(screen.queryByRole("tooltip")).toBeNull();
	},
};

export const ReducedMotionTooltip: Story = {
	args: {
		pathname: "/endpoints/endpoint-sgp",
		reducedMotionOverride: true,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const link = canvas.getByRole("link", {
			name: `Endpoint ${LONG_ENDPOINT} (endpoint-sgp)`,
		});
		const text = canvas.getByText(LONG_ENDPOINT);
		const labelViewport = text.parentElement;
		if (!labelViewport) throw new Error("Label viewport is missing.");
		await waitFor(() =>
			expect(labelViewport).toHaveAttribute("data-overflowing", "true"),
		);

		await userEvent.hover(link);
		await expect(await screen.findByRole("tooltip")).toHaveTextContent(
			LONG_ENDPOINT,
		);
		await expect(labelViewport).toHaveAttribute("data-reveal-phase", "start");
		await expect((text as HTMLElement).style.transform).toMatch(
			/^translateX\(0(?:px)?\)$/,
		);
		await userEvent.keyboard("{Escape}");
		await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
	},
};
