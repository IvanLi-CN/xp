import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const fetchNodes = vi.hoisted(() => vi.fn());
vi.mock("@/api/adminNodes", () => ({ fetchAdminNodes: fetchNodes }));
vi.mock("@/api/adminNodeVersions", () => ({ fetchAdminNodeVersions: vi.fn() }));

import { fixtureCatalog } from "@/fixture-policy/catalog";
import {
	PrimaryBackendProvider,
	usePrimaryBackend,
} from "./PrimaryBackendProvider";
import {
	clearPrimaryBackendDirectory,
	getPrimaryBackendSnapshot,
	hydratePrimaryBackendProfile,
	resetPrimaryBackendTransportForTests,
} from "./primaryBackend";
import { useBackendDirectory } from "./useBackendDirectory";

const clusterId = "cluster-directory-test";
const origin = fixtureCatalog.url.nodeA();
const node = {
	node_id: fixtureCatalog.nodeId.fixture17(),
	node_name: fixtureCatalog.nodeName.fixture18(),
	api_base_url: fixtureCatalog.url.nodeA(),
};

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

	it("initializes and loads the directory in a fresh authenticated session", async () => {
		fetchNodes.mockResolvedValue({ items: [node] });
		const client = new QueryClient({
			defaultOptions: { queries: { retry: false } },
		});
		const wrapper = ({ children }: { children: ReactNode }) => (
			<QueryClientProvider client={client}>
				<PrimaryBackendProvider>{children}</PrimaryBackendProvider>
			</QueryClientProvider>
		);
		const { result } = renderHook(
			() => {
				const primary = usePrimaryBackend();
				return useBackendDirectory({
					adminToken: "test-token",
					clusterId,
					primaryClusterId: primary.clusterId,
					primaryOrigin: primary.primaryOrigin,
					runtimeOnline: true,
					staticConsole: false,
					runtimePolicyReady: true,
					selectorOpen: false,
					refreshRuntimePolicy: async () => ({ isSuccess: false }),
				});
			},
			{ wrapper },
		);
		await waitFor(() => expect(result.current.directoryStatus).toBe("ready"));
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual([fixtureCatalog.nodeId.fixture17()]);
	});

	it("keeps the last successful list on failure and replaces it on recovery", async () => {
		fetchNodes.mockResolvedValueOnce({ items: [node] });
		const { result } = setup();
		await waitFor(() => expect(result.current.directoryStatus).toBe("ready"));
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual([fixtureCatalog.nodeId.fixture17()]);

		fetchNodes.mockRejectedValueOnce(new Error("offline"));
		act(() => result.current.refreshBackendCandidates());
		await waitFor(() => expect(result.current.directoryStatus).toBe("stale"));
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual([fixtureCatalog.nodeId.fixture17()]);

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

	it("retains the session inventory when the hook remounts offline", async () => {
		fetchNodes.mockResolvedValueOnce({ items: [node] });
		const first = setup();
		await waitFor(() =>
			expect(first.result.current.directoryStatus).toBe("ready"),
		);
		first.unmount();
		fetchNodes.mockRejectedValue(new Error("offline"));
		const second = setup();
		await waitFor(() =>
			expect(second.result.current.directoryStatus).toBe("stale"),
		);
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual([fixtureCatalog.nodeId.fixture17()]);
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
				nodeId: fixtureCatalog.nodeId.fixture17(),
				origin,
			});
		});
		act(() => clearPrimaryBackendDirectory());
		resolveInventory({ items: [node] });
		expect(await pending).toBe(false);
		expect(getPrimaryBackendSnapshot().candidates).toEqual([]);
	});
});
