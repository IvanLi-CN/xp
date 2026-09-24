import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
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
		mocks.fetchAdminNodeResources.mockReset();
		mocks.fetchAdminNodeResourceHistory.mockReset();
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
		expect(result.current.resourceQuery.data).toBe(supportedSnapshot);

		await result.current.resourceQuery.refetch();
		await waitFor(() =>
			expect(result.current.resourceQuery.isSuccess).toBe(true),
		);
	});

	it("pauses and resumes an individual history query after refresh failure", async () => {
		mocks.fetchAdminNodeResources
			.mockReset()
			.mockResolvedValue(supportedSnapshot);
		mocks.fetchAdminNodeResourceHistory.mockReset();
		let historyFailed = false;
		const historyPoint = {
			observed_at: "2026-09-01T00:00:00.000Z",
			value: 20,
		};
		mocks.fetchAdminNodeResourceHistory.mockImplementation(
			(_token: string, _nodeId: string, metric: string) =>
				metric === "cpu_busy_percent" && historyFailed
					? Promise.reject(new Error("history refresh failed"))
					: Promise.resolve({
							points: metric === "cpu_busy_percent" ? [historyPoint] : [],
						}),
		);
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
			expect(
				result.current.resourceHistoryByMetric.cpu_busy_percent,
			).toHaveLength(1),
		);
		historyFailed = true;
		await act(async () => {
			await queryClient.refetchQueries({
				queryKey: ["adminNodeResourceHistory"],
			});
		});
		await waitFor(() =>
			expect(
				result.current.resourceHistoryErrorByMetric.cpu_busy_percent,
			).toBeDefined(),
		);
		expect(
			result.current.resourceHistoryByMetric.cpu_busy_percent,
		).toHaveLength(1);

		historyFailed = false;
		await act(async () => {
			await queryClient.refetchQueries({
				queryKey: ["adminNodeResourceHistory"],
			});
		});
		await waitFor(() =>
			expect(
				result.current.resourceHistoryErrorByMetric.cpu_busy_percent,
			).toBeNull(),
		);
	});

	it("does not reuse a snapshot after the page session unmounts", async () => {
		mocks.fetchAdminNodeResources
			.mockReset()
			.mockResolvedValue(supportedSnapshot);
		mocks.fetchAdminNodeResourceHistory
			.mockReset()
			.mockResolvedValue({ points: [] });
		const queryClient = new QueryClient({
			defaultOptions: {
				queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
			},
		});
		const wrapper = wrapperFor(queryClient);
		const props = {
			adminToken: "admin-token",
			nodeId: "node-a",
			nodesAvailable: true,
			isOnline: true,
			activeTab: "resources",
			selectedRuntimeRole: null,
		} as const;

		const first = renderHook(() => useNodeResourceQueries(props), { wrapper });
		await waitFor(() =>
			expect(first.result.current.resourceQuery.isSuccess).toBe(true),
		);
		first.unmount();

		const second = renderHook(() => useNodeResourceQueries(props), { wrapper });
		expect(second.result.current.resourceQuery.isPending).toBe(true);
		await waitFor(() =>
			expect(second.result.current.resourceQuery.isSuccess).toBe(true),
		);
		expect(mocks.fetchAdminNodeResources).toHaveBeenCalledTimes(2);
	});
});
