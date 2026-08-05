import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockReadPersistedQuerySnapshot, mockWritePersistedQuerySnapshot } =
	vi.hoisted(() => ({
		mockReadPersistedQuerySnapshot: vi.fn(),
		mockWritePersistedQuerySnapshot: vi.fn(),
	}));

vi.mock("./queryPersistence", () => ({
	readPersistedQuerySnapshot: mockReadPersistedQuerySnapshot,
	writePersistedQuerySnapshot: mockWritePersistedQuerySnapshot,
}));

import { useQueryWithOfflineFallback } from "./useQueryWithOfflineFallback";

describe("useQueryWithOfflineFallback", () => {
	beforeEach(() => {
		mockReadPersistedQuerySnapshot.mockReset();
		mockWritePersistedQuerySnapshot.mockReset();
	});

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

	it("does not read stale fallback data while an online query is fetching", async () => {
		const query = {
			data: undefined,
			dataUpdatedAt: 0,
			isError: false,
			isLoading: true,
			fetchStatus: "fetching" as const,
		};
		const { result } = renderHook(() =>
			useQueryWithOfflineFallback(["adminNodesRuntime", "admin-token"], query),
		);

		await waitFor(() => expect(result.current.data).toBeUndefined());
		expect(mockReadPersistedQuerySnapshot).not.toHaveBeenCalled();
	});
});
