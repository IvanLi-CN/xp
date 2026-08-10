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
		expect(resourceList).toHaveClass("h-[20rem]");
		expect(resourceList).toHaveClass("overflow-hidden");
		expect(resourceList).not.toHaveClass("overflow-y-auto");
		expect(
			resourceList.querySelector("[data-radix-scroll-area-viewport]"),
		).toBeInTheDocument();

		fireEvent.click(screen.getByRole("link", { name: "Dashboard" }));
		expect(onNavigate).toHaveBeenCalledWith("/");
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
		expect(activeUser).toHaveAttribute("aria-current", "page");
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
