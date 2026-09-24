import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { supportedSnapshot } from "../components/ResourceSnapshotPanel.stories";
import { useNodeResourceQueries } from "./useNodeResourceQueries";

const mocks = vi.hoisted(() => ({
	fetchAdminNodeResourceHistory: vi.fn(),
	fetchAdminNodeResources: vi.fn(),
}));

vi.mock("../api/adminResources", async () => {
	const actual = await vi.importActual<typeof import("../api/adminResources")>(
		"../api/adminResources",
	);
	return {
		...actual,
		fetchAdminNodeResourceHistory: mocks.fetchAdminNodeResourceHistory,
		fetchAdminNodeResources: mocks.fetchAdminNodeResources,
	};
});

vi.mock("../api/useApiCompatibility", () => ({
	useApiCapability: () => ({ available: true }),
}));

function wrapperFor(queryClient: QueryClient) {
	return ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
	);
}

describe("useNodeResourceQueries lifecycle", () => {
	it("retains data, pauses failed polling, and resumes after a successful retry", async () => {
		const error = new Error("raw transport details stay out of the UI");
		mocks.fetchAdminNodeResources
			.mockResolvedValueOnce(supportedSnapshot)
			.mockRejectedValueOnce(error)
			.mockResolvedValueOnce(supportedSnapshot);
		mocks.fetchAdminNodeResourceHistory.mockResolvedValue({ points: [] });
		const queryClient = new QueryClient({
			defaultOptions: {
				queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
			},
		});

		const { result } = renderHook(
			() =>
				useNodeResourceQueries({
					adminToken: "admin-token",
					nodeId: "node-a",
					nodesAvailable: true,
					isOnline: true,
					activeTab: "resources",
					selectedRuntimeRole: null,
				}),
			{ wrapper: wrapperFor(queryClient) },
		);

		await waitFor(() =>
			expect(result.current.resourceQuery.isSuccess).toBe(true),
		);
		await result.current.resourceQuery.refetch();
		await waitFor(() =>
			expect(result.current.resourceQuery.isError).toBe(true),
		);
		const query = queryClient.getQueryCache().find({
			queryKey: ["adminNodeResources", "admin-token", "node-a"],
		});
		const refetchInterval = (
			query?.options as {
				refetchInterval?: (query: unknown) => number | false;
			}
		)?.refetchInterval;
		expect(result.current.resourceQuery.data).toBe(supportedSnapshot);
		expect(typeof refetchInterval).toBe("function");
		if (typeof refetchInterval === "function" && query) {
			expect(refetchInterval(query)).toBe(false);
		}

		await result.current.resourceQuery.refetch();
		await waitFor(() =>
			expect(result.current.resourceQuery.isSuccess).toBe(true),
		);
		if (typeof refetchInterval === "function" && query) {
			expect(refetchInterval(query)).toBe(15_000);
		}
	});
});
