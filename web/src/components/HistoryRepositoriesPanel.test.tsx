import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { createQueryClient } from "../queryClient";
import { HistoryRepositoriesPanel } from "./HistoryRepositoriesPanel";

vi.mock("../api/useApiCompatibility", () => ({
	useApiCapability: () => ({
		available: true,
		unavailable: false,
		reason: null,
	}),
}));
vi.mock("../offline/appRuntime", () => ({
	useAppRuntime: () => ({ isOnline: false, isReadOnly: false }),
}));
vi.mock("../offline/useQueryWithOfflineFallback", () => ({
	useQueryWithOfflineFallback: () => ({
		data: {
			configured: true,
			partial: false,
			unreachable_node_ids: [],
			items: [],
		},
		isError: false,
		isLoading: false,
		isFetching: false,
		dataUpdatedAt: 1,
		refetch: vi.fn(),
	}),
}));
vi.mock("./HistoryRepositoryStatus", () => ({
	RepositoryStatusSummary: () => <div>repository summary</div>,
}));
vi.mock("./HistoryRepositoryMembershipEditor", () => ({
	HistoryRepositoryMembershipEditor: () => <div>membership editor</div>,
}));

describe("HistoryRepositoriesPanel", () => {
	it("labels cached repository status as offline when its query is paused", () => {
		render(
			<QueryClientProvider client={createQueryClient()}>
				<HistoryRepositoriesPanel adminToken="admin-token" nodes={[]} />
			</QueryClientProvider>,
		);

		expect(screen.getByText("Offline repository status")).toBeVisible();
		expect(screen.getByText("repository summary")).toBeVisible();
	});
});
