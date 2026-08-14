import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { fetchAdminRepositoryHistory } from "../api/adminRepositoryHistory";
import { createQueryClient } from "../queryClient";
import { NodeRepositoryQuality } from "./NodeRepositoryHistoryQuality";

vi.mock("../api/adminRepositoryHistory");
vi.mock("../api/useApiCompatibility", () => ({
	useApiCapability: () => ({
		available: true,
		unavailable: false,
		reason: null,
	}),
}));
vi.mock("../offline/appRuntime", () => ({
	useAppRuntime: () => ({ isOnline: true, isReadOnly: false }),
}));
vi.mock("../offline/useQueryWithOfflineFallback", () => ({
	useQueryWithOfflineFallback: (
		_queryKey: readonly unknown[],
		query: unknown,
	) => query,
}));
vi.mock("./HistoryRepositoryStatus", () => ({
	RepositoryQueryQuality: ({
		onNextPage,
	}: {
		onNextPage?: () => void;
	}) => (
		<button type="button" onClick={onNextPage}>
			Next page
		</button>
	),
}));

const response = {
	repository: "repository-a",
	completeness: "complete" as const,
	coverage: null,
	watermarks: [],
	gaps: [],
	clock_skew_seconds: 0,
	records: [],
	records_truncated: true,
	next_page_cursor: "100",
};

function page(nodeId: string) {
	const queryClient = createQueryClient();
	return render(
		<QueryClientProvider client={queryClient}>
			<NodeRepositoryQuality adminToken="admin-token" nodeId={nodeId} />
		</QueryClientProvider>,
	);
}

describe("NodeRepositoryQuality", () => {
	it("starts a changed node at the first history page", async () => {
		vi.mocked(fetchAdminRepositoryHistory).mockResolvedValue(response);
		const view = page("node-a");
		await waitFor(() => {
			expect(fetchAdminRepositoryHistory).toHaveBeenCalledWith(
				"admin-token",
				expect.objectContaining({ subjectNodeId: "node-a" }),
				expect.any(AbortSignal),
			);
		});
		fireEvent.click(await screen.findByRole("button", { name: "Next page" }));
		await waitFor(() => {
			expect(fetchAdminRepositoryHistory).toHaveBeenLastCalledWith(
				"admin-token",
				expect.objectContaining({ pageCursor: "100", subjectNodeId: "node-a" }),
				expect.any(AbortSignal),
			);
		});
		const callsBeforeNodeChange = vi.mocked(fetchAdminRepositoryHistory).mock
			.calls.length;

		view.rerender(
			<QueryClientProvider client={createQueryClient()}>
				<NodeRepositoryQuality adminToken="admin-token" nodeId="node-b" />
			</QueryClientProvider>,
		);
		await waitFor(() => {
			expect(fetchAdminRepositoryHistory).toHaveBeenLastCalledWith(
				"admin-token",
				expect.objectContaining({
					pageCursor: undefined,
					subjectNodeId: "node-b",
				}),
				expect.any(AbortSignal),
			);
		});
		const newNodeRequests = vi
			.mocked(fetchAdminRepositoryHistory)
			.mock.calls.slice(callsBeforeNodeChange)
			.filter(([, query]) => query.subjectNodeId === "node-b");
		expect(newNodeRequests).not.toHaveLength(0);
		expect(newNodeRequests.every(([, query]) => !query.pageCursor)).toBe(true);
	});
});
