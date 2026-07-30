import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { mockReadPersistedQuerySnapshot } = vi.hoisted(() => ({
	mockReadPersistedQuerySnapshot: vi.fn(),
}));

vi.mock("./queryPersistence", () => ({
	readPersistedQuerySnapshot: mockReadPersistedQuerySnapshot,
}));

import { useQueryWithOfflineFallback } from "./useQueryWithOfflineFallback";

describe("useQueryWithOfflineFallback", () => {
	it("reads a persisted snapshot once for equivalent query keys", async () => {
		const snapshot = {
			data: { items: ["cached-node"] },
			dataUpdatedAt: 1_753_862_400_000,
		};
		mockReadPersistedQuerySnapshot.mockResolvedValue(snapshot);

		const query = {
			data: undefined,
			dataUpdatedAt: 0,
			isError: true,
			isLoading: false,
		};
		const { result, rerender } = renderHook(() =>
			useQueryWithOfflineFallback(["adminNodesRuntime", "admin-token"], query),
		);

		await waitFor(() => {
			expect(result.current.data).toEqual(snapshot.data);
		});
		rerender();

		expect(mockReadPersistedQuerySnapshot).toHaveBeenCalledTimes(1);
	});
});
