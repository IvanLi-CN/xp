import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import {
	NODE_RESOURCE_HISTORY_METRICS,
	RUNTIME_RESOURCE_HISTORY_METRICS,
} from "../api/adminResources";
import { fixtureCatalog } from "../fixture-policy/catalog";
import * as resourceMonitoringFixtures from "../fixture-policy/resourceMonitoring";
import { useNodeResourceQueries } from "./useNodeResourceQueries";

const supportedSnapshot = resourceMonitoringFixtures.supportedResourceSnapshot;

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
					nodeId: fixtureCatalog.identifier.nodePrimary(),
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
			observed_at: fixtureCatalog.timestamp.t20260901T000000(),
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
					nodeId: fixtureCatalog.identifier.nodePrimary(),
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

	it("pauses and resumes runtime history independently", async () => {
		mocks.fetchAdminNodeResources
			.mockReset()
			.mockResolvedValue(supportedSnapshot);
		mocks.fetchAdminNodeResourceHistory.mockReset();
		let runtimeFailed = false;
		const runtimePoint = {
			observed_at: fixtureCatalog.timestamp.t20260901T000000(),
			value: 8,
		};
		mocks.fetchAdminNodeResourceHistory.mockImplementation(
			(
				_token: string,
				_nodeId: string,
				metric: string,
				_signal: AbortSignal | undefined,
				role: string | undefined,
			) =>
				role === "xray" && metric === "cpu_percent" && runtimeFailed
					? Promise.reject(new Error("runtime history refresh failed"))
					: Promise.resolve({
							points:
								role === "xray" && metric === "cpu_percent"
									? [runtimePoint]
									: [],
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
					nodeId: fixtureCatalog.identifier.nodePrimary(),
					nodesAvailable: true,
					isOnline: true,
					activeTab: "resources",
					selectedRuntimeRole: "xray",
				}),
			{ wrapper: wrapperFor(queryClient) },
		);

		await waitFor(() =>
			expect(result.current.runtimeHistoryByMetric.cpu_percent).toHaveLength(1),
		);
		runtimeFailed = true;
		await act(async () => {
			await queryClient.refetchQueries({
				queryKey: ["adminNodeRuntimeResourceHistory"],
			});
		});
		await waitFor(() =>
			expect(
				result.current.runtimeHistoryErrorByMetric.cpu_percent,
			).toBeDefined(),
		);
		expect(result.current.runtimeHistoryByMetric.cpu_percent).toHaveLength(1);

		runtimeFailed = false;
		await act(async () => {
			await queryClient.refetchQueries({
				queryKey: ["adminNodeRuntimeResourceHistory"],
			});
		});
		await waitFor(() =>
			expect(result.current.runtimeHistoryErrorByMetric.cpu_percent).toBeNull(),
		);
	});

	it("does not reuse or retain resource queries after page session unmounts", async () => {
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
			nodeId: fixtureCatalog.identifier.nodePrimary(),
			nodesAvailable: true,
			isOnline: true,
			activeTab: "resources",
			selectedRuntimeRole: "xray",
		} as const;

		const resourceQueryKeys = () =>
			queryClient
				.getQueryCache()
				.getAll()
				.filter(
					({ queryKey }) =>
						queryKey[0] === "adminNodeResources" ||
						queryKey[0] === "adminNodeResourceHistory" ||
						queryKey[0] === "adminNodeRuntimeResourceHistory",
				)
				.map(({ queryKey }) => queryKey);

		for (let sessionIndex = 0; sessionIndex < 3; sessionIndex += 1) {
			const session = renderHook(() => useNodeResourceQueries(props), {
				wrapper,
			});
			if (sessionIndex > 0) {
				expect(session.result.current.resourceQuery.isPending).toBe(true);
			}
			await waitFor(() =>
				expect(session.result.current.resourceQuery.isSuccess).toBe(true),
			);

			const keys = resourceQueryKeys();
			expect(keys).toHaveLength(
				1 +
					NODE_RESOURCE_HISTORY_METRICS.length +
					RUNTIME_RESOURCE_HISTORY_METRICS.length,
			);
			for (const key of keys) {
				expect(key[1]).toBe(props.adminToken);
				expect(key[2]).toBe(props.nodeId);
			}
			expect(keys.find((key) => key[0] === "adminNodeResources")).toEqual([
				"adminNodeResources",
				props.adminToken,
				props.nodeId,
			]);

			session.unmount();
			expect(resourceQueryKeys()).toHaveLength(0);
		}
		expect(mocks.fetchAdminNodeResources).toHaveBeenCalledTimes(3);
	});
});
