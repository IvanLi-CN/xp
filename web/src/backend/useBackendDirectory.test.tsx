import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const fetchNodes = vi.hoisted(() => vi.fn());
vi.mock("@/api/adminNodes", () => ({ fetchAdminNodes: fetchNodes }));
vi.mock("@/api/adminNodeVersions", () => ({ fetchAdminNodeVersions: vi.fn() }));

import {
	clearPrimaryBackendDirectory,
	getPrimaryBackendSnapshot,
	hydratePrimaryBackendProfile,
	resetPrimaryBackendTransportForTests,
} from "./primaryBackend";
import { useBackendDirectory } from "./useBackendDirectory";

const clusterId = "cluster-directory-test";
const origin = "https://node.xp.example";
const node = { node_id: "node-a", node_name: "Node A", api_base_url: origin };

function setup() {
	const client = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	const wrapper = ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={client}>{children}</QueryClientProvider>
	);
	hydratePrimaryBackendProfile(clusterId);
	const result = renderHook(
		() =>
			useBackendDirectory({
				adminToken: "test-token",
				clusterId,
				primaryClusterId: clusterId,
				primaryOrigin: window.location.origin,
				runtimeOnline: true,
				staticConsole: false,
				runtimePolicyReady: true,
				selectorOpen: false,
				refreshRuntimePolicy: async () => ({ isSuccess: false }),
			}),
		{ wrapper },
	);
	return { ...result, client };
}

describe("backend directory refresh", () => {
	beforeEach(() => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
		fetchNodes.mockReset();
	});
	afterEach(() => resetPrimaryBackendTransportForTests());

	it("keeps the last successful list on failure and replaces it on recovery", async () => {
		fetchNodes.mockResolvedValueOnce({ items: [node] });
		const { result } = setup();
		await waitFor(() => expect(result.current.directoryStatus).toBe("ready"));
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual(["node-a"]);

		fetchNodes.mockRejectedValueOnce(new Error("offline"));
		act(() => result.current.refreshBackendCandidates());
		await waitFor(() => expect(result.current.directoryStatus).toBe("stale"));
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual(["node-a"]);

		fetchNodes.mockResolvedValueOnce({ items: [] });
		act(() => result.current.refreshBackendCandidates());
		await waitFor(() => expect(result.current.directoryStatus).toBe("ready"));
		expect(getPrimaryBackendSnapshot().candidates).toEqual([]);
	});

	it("reports an error when no inventory has ever been retrieved", async () => {
		fetchNodes.mockRejectedValue(new Error("offline"));
		const { result } = setup();
		await waitFor(() => expect(result.current.directoryStatus).toBe("error"));
	});

	it("does not restore a delayed revalidation after logout", async () => {
		fetchNodes.mockResolvedValueOnce({ items: [node] });
		const { result } = setup();
		await waitFor(() => expect(result.current.directoryStatus).toBe("ready"));
		let resolveInventory!: (value: { items: (typeof node)[] }) => void;
		fetchNodes.mockImplementationOnce(
			() =>
				new Promise((resolve) => {
					resolveInventory = resolve;
				}),
		);
		let pending!: Promise<boolean>;
		act(() => {
			pending = result.current.revalidateBackendCandidate({
				nodeId: node.node_id,
				origin,
			});
		});
		act(() => clearPrimaryBackendDirectory());
		resolveInventory({ items: [node] });
		expect(await pending).toBe(false);
		expect(getPrimaryBackendSnapshot().candidates).toEqual([]);
	});
});
