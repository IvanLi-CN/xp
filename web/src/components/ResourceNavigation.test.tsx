import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
	ResourceNavigation,
	type ResourceNavigationGroup,
} from "./ResourceNavigation";

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
						id: "node-01",
						label: "Node 01",
						href: "/nodes/node-01",
						ariaLabel: "Node 01 (node-01)",
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
						id: "endpoint-01",
						label: "Endpoint 01",
						href: "/endpoints/endpoint-01",
						ariaLabel: "Endpoint 01 (endpoint-01)",
					},
				],
			},
			{
				id: "users",
				label: "Users",
				href: "/users",
				icon: "tabler:users",
				children: Array.from({ length: 12 }, (_, index) => {
					const number = String(index + 1).padStart(2, "0");
					return {
						id: `user-${number}`,
						label: `User ${number}`,
						href: `/users/user-${number}`,
						ariaLabel: `User ${number} (user-${number})`,
					};
				}),
			},
		],
	},
];

describe("<ResourceNavigation />", () => {
	afterEach(() => {
		vi.restoreAllMocks();
	});

	it("starts collapsed at the exact index route and uses the shared scroll area", () => {
		const onNavigate = vi.fn();
		render(
			<ResourceNavigation
				ariaLabel="Primary navigation"
				groups={groups}
				pathname="/users"
				onNavigate={onNavigate}
			/>,
		);

		expect(screen.queryByText("User 01")).toBeNull();
		fireEvent.click(screen.getByRole("button", { name: "Expand Users" }));
		expect(screen.getByRole("link", { name: "Users" })).toHaveAttribute(
			"aria-current",
			"page",
		);

		expect(screen.getByText("User 01")).toBeInTheDocument();
		expect(screen.getByText("User 12")).toBeInTheDocument();
		const resourceList = screen.getByTestId("resource-list-users");
		expect(resourceList).toHaveClass("max-h-[20rem]");
		expect(resourceList).not.toHaveClass("h-[20rem]");
		expect(resourceList).toHaveClass("overflow-hidden");
		expect(resourceList).not.toHaveClass("overflow-y-auto");
		expect(
			resourceList.querySelector("[data-radix-scroll-area-viewport]"),
		).toBeInTheDocument();
		expect(resourceList.querySelector("ul")).toHaveClass("w-0", "min-w-full");

		fireEvent.click(screen.getByRole("link", { name: "Dashboard" }));
		expect(onNavigate).toHaveBeenCalledWith("/");
	});

	it("keeps only one resource group expanded and allows every group to close", () => {
		const onResourceRequested = vi.fn();
		render(
			<ResourceNavigation
				ariaLabel="Primary navigation"
				groups={groups}
				pathname="/users"
				onNavigate={vi.fn()}
				onResourceRequested={onResourceRequested}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Expand Nodes" }));
		expect(screen.getByText("Node 01")).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Collapse Nodes" }),
		).toHaveAttribute("aria-expanded", "true");

		fireEvent.click(screen.getByRole("button", { name: "Expand Endpoints" }));
		expect(screen.queryByText("Node 01")).toBeNull();
		expect(screen.getByText("Endpoint 01")).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Expand Nodes" }),
		).toHaveAttribute("aria-expanded", "false");

		fireEvent.click(screen.getByRole("button", { name: "Collapse Endpoints" }));
		expect(screen.queryByText("Endpoint 01")).toBeNull();
		expect(
			screen.getByRole("button", { name: "Expand Endpoints" }),
		).toHaveAttribute("aria-expanded", "false");
		expect(onResourceRequested).toHaveBeenCalledTimes(2);
		expect(onResourceRequested).toHaveBeenNthCalledWith(1, "nodes");
		expect(onResourceRequested).toHaveBeenNthCalledWith(2, "endpoints");
	});

	it("opens nested routes, scrolls active child, and uses guarded navigation", async () => {
		const onNavigate = vi.fn();
		const onResourceNavigate = vi.fn();
		const scrollIntoView = vi
			.spyOn(HTMLElement.prototype, "scrollIntoView")
			.mockImplementation(() => undefined);
		render(
			<ResourceNavigation
				ariaLabel="Primary navigation"
				groups={groups}
				pathname="/users/user-12"
				onNavigate={onNavigate}
				onResourceNavigate={onResourceNavigate}
			/>,
		);

		const activeUser = screen.getByRole("link", {
			name: "User 12 (user-12)",
		});
		expect(
			screen.getByRole("button", { name: "Collapse Users" }),
		).toHaveAttribute("aria-expanded", "true");
		expect(activeUser).toHaveClass("bg-primary/10");
		expect(activeUser).toHaveClass("rounded-full");
		expect(activeUser).toHaveAttribute("aria-current", "page");
		expect(activeUser).not.toHaveAttribute("title");
		await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());

		fireEvent.click(activeUser);
		expect(onResourceNavigate).toHaveBeenCalledWith("/users/user-12");
		expect(onNavigate).not.toHaveBeenCalled();
	});

	it("uses instance-specific disclosures and requests data outside the state updater", () => {
		const onResourceRequested = vi.fn();
		const { container } = render(
			<>
				<ResourceNavigation
					ariaLabel="Desktop navigation"
					groups={groups}
					pathname="/users"
					onNavigate={vi.fn()}
					onResourceRequested={onResourceRequested}
				/>
				<ResourceNavigation
					ariaLabel="Mobile navigation"
					groups={groups}
					pathname="/users"
					onNavigate={vi.fn()}
				/>
			</>,
		);

		const disclosureIds = [
			...container.querySelectorAll("[aria-controls]"),
		].map((button) => button.getAttribute("aria-controls"));
		expect(new Set(disclosureIds).size).toBe(disclosureIds.length);

		const expandUsers = screen.getAllByRole("button", {
			name: "Expand Users",
		})[0];
		if (!expandUsers) throw new Error("Users disclosure is missing.");
		fireEvent.click(expandUsers);
		expect(onResourceRequested).toHaveBeenCalledTimes(1);
		expect(onResourceRequested).toHaveBeenCalledWith("users");
	});
});
