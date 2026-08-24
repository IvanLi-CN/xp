import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
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
	RepositoryStatusSummary: ({
		nodeNames,
	}: {
		nodeNames?: Readonly<Record<string, string>>;
	}) => (
		<div>repository summary {nodeNames ? JSON.stringify(nodeNames) : ""}</div>
	),
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
		expect(screen.getByText(/repository summary/)).toBeVisible();
	});

	it("passes the node directory to repository status", () => {
		render(
			<QueryClientProvider client={createQueryClient()}>
				<HistoryRepositoriesPanel
					adminToken="admin-token"
					nodes={[
						{
							node_id: fixtureCatalog.nodeId.fixture32(),
							node_name: fixtureCatalog.nodeName.fixture33(),
						},
					]}
				/>
			</QueryClientProvider>,
		);

		expect(screen.getByText(/node-fixture-32.*fixture-node-33/)).toBeVisible();
	});
});
