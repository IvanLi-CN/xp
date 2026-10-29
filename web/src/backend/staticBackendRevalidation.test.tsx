// @vitest-environment jsdom
// @vitest-environment-options {"url":"https://xp.ivanli.cc"}
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { fetchNodes, fetchPolicy } = vi.hoisted(() => ({
	fetchNodes: vi.fn(),
	fetchPolicy: vi.fn(),
}));
vi.mock("@/api/adminNodes", async (original) => ({
	...(await original<typeof import("@/api/adminNodes")>()),
	fetchAdminNodes: fetchNodes,
}));
vi.mock("./primaryBackend", async (original) => ({
	...(await original<typeof import("./primaryBackend")>()),
	readSwitchCandidateResource: (args: { path: string }) =>
		args.path === "/api/admin/nodes" ? fetchNodes() : fetchPolicy(),
}));
vi.mock("@/api/adminNodeVersions", () => ({ fetchAdminNodeVersions: vi.fn() }));
vi.mock("@/api/runtimePolicy", async (original) => ({
	...(await original<typeof import("@/api/runtimePolicy")>()),
	fetchRuntimePolicy: fetchPolicy,
}));

import { fixtureCatalog } from "@/fixture-policy/catalog";
import {
	getPrimaryBackendSnapshot,
	hydrateBackendVersions,
	hydratePrimaryBackendProfile,
	hydrateStaticRuntimePolicy,
	resetPrimaryBackendTransportForTests,
} from "./primaryBackend";
import { useBackendDirectory } from "./useBackendDirectory";

const clusterId = fixtureCatalog.cluster.fixture84();
const origin = fixtureCatalog.url.nodeA();
const node = {
	node_id: fixtureCatalog.nodeId.fixture17(),
	node_name: fixtureCatalog.nodeName.fixture18(),
	api_base_url: fixtureCatalog.url.nodeA(),
	access_host: fixtureCatalog.host.primary(),
	quota_limit_bytes: 0,
	quota_reset: { policy: "unlimited" as const },
};

function setup(runtimePolicyReady = true) {
	const client = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	const wrapper = ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={client}>{children}</QueryClientProvider>
	);
	hydrateStaticRuntimePolicy(clusterId, [origin], {
		activate: true,
		policyId: "policy-active",
	});
	hydratePrimaryBackendProfile(clusterId, [node]);
	return renderHook(
		() =>
			useBackendDirectory({
				adminToken: "test-token",
				clusterId,
				primaryClusterId: clusterId,
				primaryOrigin: window.location.origin,
				runtimeOnline: true,
				staticConsole: true,
				runtimePolicyReady,
				selectorOpen: false,
				refreshRuntimePolicy: async () => ({ isSuccess: false }),
			}),
		{ wrapper },
	);
}

describe("static candidate revalidation", () => {
	beforeEach(() => {
		window.localStorage.clear();
		window.sessionStorage.clear();
		resetPrimaryBackendTransportForTests();
		fetchNodes.mockReset();
		fetchPolicy.mockReset();
		fetchNodes.mockResolvedValue({ items: [node] });
	});
	afterEach(() => resetPrimaryBackendTransportForTests());

	it.each(["removed", "unavailable"])(
		"does not send the inventory token after the refreshed policy marks the node %s",
		async (state) => {
			const { result } = setup();
			await waitFor(() => expect(result.current.directoryStatus).toBe("ready"));
			fetchNodes.mockClear();
			fetchPolicy.mockResolvedValue({
				cluster_id: fixtureCatalog.cluster.fixture84(),
				policy_id: "policy-new",
				issued_at: new Date(Date.now() - 1_000).toISOString(),
				expires_at: new Date(Date.now() + 60_000).toISOString(),
				api_origins: state === "removed" ? [] : [origin],
				node_states:
					state === "removed"
						? []
						: [{ origin, state: "temporarily_unavailable" }],
			});
			expect(
				await result.current.revalidateBackendCandidate({
					nodeId: fixtureCatalog.nodeId.fixture17(),
					origin,
				}),
			).toBe(false);
			expect(fetchNodes).not.toHaveBeenCalled();
		},
	);

	it("still verifies inventory when the refreshed policy permits the active origin", async () => {
		const { result } = setup();
		await waitFor(() => expect(result.current.directoryStatus).toBe("ready"));
		fetchNodes.mockClear();
		fetchPolicy.mockResolvedValue({
			cluster_id: fixtureCatalog.cluster.fixture84(),
			policy_id: "policy-new",
			issued_at: new Date(Date.now() - 1_000).toISOString(),
			expires_at: new Date(Date.now() + 60_000).toISOString(),
			api_origins: [origin],
			node_states: [{ origin, state: "allowed" }],
		});
		expect(
			await result.current.revalidateBackendCandidate({
				nodeId: fixtureCatalog.nodeId.fixture17(),
				origin,
			}),
		).toBe(true);
		expect(fetchNodes).toHaveBeenCalledOnce();
	});

	it("hides the previous version when static policy is not ready", () => {
		const { result } = setup(false);
		hydrateBackendVersions([
			{
				node_id: fixtureCatalog.nodeId.fixture17(),
				xp_version: "3.45.0",
				error: null,
			},
		]);
		expect(getPrimaryBackendSnapshot().candidates[0]?.serviceVersion).toBe(
			"3.45.0",
		);
		result.current.refreshBackendCandidates();
		expect(
			getPrimaryBackendSnapshot().candidates[0]?.serviceVersion,
		).toBeNull();
	});
});
