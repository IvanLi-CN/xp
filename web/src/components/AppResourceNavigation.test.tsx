import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { createQueryClient } from "../queryClient";
import { AppResourceNavigation } from "./AppResourceNavigation";

const groups = [
	{
		title: "Nav",
		items: [{ label: "Users", to: "/users", icon: "tabler:users" }],
	},
];

describe("<AppResourceNavigation />", () => {
	it("shows a retryable compatibility error instead of permanent loading", () => {
		const onRetryCompatibility = vi.fn();
		render(
			<QueryClientProvider client={createQueryClient()}>
				<AppResourceNavigation
					adminToken="admintoken"
					compatibility={null}
					compatibilityError="Compatibility request failed"
					compatibilityPending={false}
					groups={groups}
					pathname="/users"
					onNavigate={vi.fn()}
					onResourceNavigate={vi.fn()}
					onRetryCompatibility={onRetryCompatibility}
				/>
			</QueryClientProvider>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Expand Users" }));
		expect(screen.queryByText("Loading users...")).toBeNull();
		expect(screen.getByText("Unable to load")).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Retry Users" }));
		expect(onRetryCompatibility).toHaveBeenCalledTimes(1);
	});
});
