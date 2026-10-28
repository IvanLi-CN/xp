import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createMockApi } from "../../.storybook/mocks/apiMock";
import { fetchAdminNodeHistory } from "../api/adminNodeHistory";
import {
	fetchAdminNodeRuntime,
	startNodeRuntimeEvents,
} from "../api/adminNodeRuntime";
import {
	type AdminNode,
	fetchAdminNode,
	fetchAdminNodeMihomoResourcePolicy,
} from "../api/adminNodes";
import { fetchAdminNodeTraffic } from "../api/adminTraffic";
import { ObjectNavigationGuardProvider } from "../components/ObjectNavigationGuard";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { createQueryClient } from "../queryClient";
import { NodeDetailsPage } from "./NodeDetailsPage";

const { mockNavigate, mockReadAdminToken, mockRouteParams, mockRouteSearch } =
	vi.hoisted(() => ({
		mockNavigate: vi.fn(),
		mockReadAdminToken: vi.fn(() => "admintoken"),
		mockRouteParams: {} as { nodeId: string },
		mockRouteSearch: {} as { tab?: string },
	}));

vi.mock("@tanstack/react-router", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("@tanstack/react-router")>();
	return {
		...actual,
		Link: ({
			children,
			to,
			...rest
		}: { children: React.ReactNode; to?: string }) => (
			<a href={to ?? "#"} {...rest}>
				{children}
			</a>
		),
		useNavigate: () => mockNavigate,
		useParams: () => mockRouteParams,
		useSearch: () => mockRouteSearch,
	};
});

vi.mock("../api/adminNodes");
vi.mock("../api/adminNodeHistory");
vi.mock("../api/adminNodeRuntime");
vi.mock("../api/adminTraffic");
vi.mock("../components/NodeRepositoryHistoryQuality", () => ({
	NodeRepositoryQuality: () => null,
}));
vi.mock("../hooks/useNodeResourceQueries", () => ({
	useNodeResourceQueries: () => ({
		resourceCapability: { available: true, unavailable: false, reason: null },
		resourceQuery: {
			data: undefined,
			dataUpdatedAt: 0,
			isError: false,
			isFetching: false,
			isLoading: false,
			refetch: vi.fn(),
		},
		resourceTabProps: {},
	}),
}));
vi.mock("../components/auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../components/auth")>();
	return { ...actual, readAdminToken: mockReadAdminToken };
});

function node(nodeId: string): AdminNode {
	const secondary = nodeId === fixtureCatalog.nodeId.fixture36();
	return {
		node_id: secondary
			? fixtureCatalog.nodeId.fixture36()
			: fixtureCatalog.nodeId.fixture32(),
		node_name: secondary
			? fixtureCatalog.nodeName.fixture37()
			: fixtureCatalog.nodeName.fixture33(),
		api_base_url: secondary
			? fixtureCatalog.service.fixture38()
			: fixtureCatalog.service.fixture34(),
		access_host: secondary
			? fixtureCatalog.host.fixture39()
			: fixtureCatalog.host.fixture35(),
		quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
		quota_reset: fixtureCatalog.quota.resetNode(),
	};
}

function renderPage() {
	const queryClient = createQueryClient();
	const page = () => (
		<QueryClientProvider client={queryClient}>
			<ObjectNavigationGuardProvider>
				<UiPrefsProvider>
					<ToastProvider>
						<NodeDetailsPage />
					</ToastProvider>
				</UiPrefsProvider>
			</ObjectNavigationGuardProvider>
		</QueryClientProvider>
	);
	const result = render(page());
	return { ...result, rerenderPage: () => result.rerender(page()) };
}

describe("<NodeDetailsPage /> traffic fixture", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		sessionStorage.clear();
		vi.spyOn(Date, "now").mockReturnValue(Date.parse("2026-03-08T00:59:30Z"));
		mockReadAdminToken.mockReturnValue("admintoken");
		mockRouteParams.nodeId = fixtureCatalog.nodeId.fixture32();
		mockRouteSearch.tab = "traffic";
		vi.mocked(fetchAdminNode).mockImplementation(async (_token, nodeId) =>
			node(nodeId),
		);
		vi.mocked(fetchAdminNodeMihomoResourcePolicy).mockResolvedValue({
			node_id: fixtureCatalog.nodeId.fixture32(),
			deployment_default_cidrs: [],
			override_cidrs: null,
			effective_cidrs: [],
			source: "deployment_default",
			status: "healthy",
			error: null,
		});
		vi.mocked(fetchAdminNodeRuntime).mockResolvedValue(undefined as never);
		vi.mocked(fetchAdminNodeHistory).mockResolvedValue(undefined as never);
		vi.mocked(startNodeRuntimeEvents).mockReturnValue({ close: vi.fn() });
	});

	afterEach(() => {
		cleanup();
		vi.restoreAllMocks();
	});

	it("keeps cached traffic data-bearing across a fixture-node round trip", async () => {
		const demoApi = createMockApi({ trafficNowMs: Date.now() });
		vi.mocked(fetchAdminNodeTraffic).mockImplementation(
			async (_token, nodeId, window) => {
				const response = await demoApi.handle(
					new Request(
						`http://localhost/api/admin/nodes/${encodeURIComponent(nodeId)}/traffic?window=${window}`,
					),
				);
				if (!response.ok)
					throw new Error(`traffic mock failed: ${response.status}`);
				return response.json();
			},
		);
		const page = renderPage();

		await waitFor(() => {
			expect(fetchAdminNodeTraffic).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture32(),
				"24h",
				expect.any(AbortSignal),
			);
			expect(screen.getByText(/Last sample:/)).toBeTruthy();
			expect(screen.queryByText("No traffic samples yet")).toBeNull();
		});

		mockRouteParams.nodeId = fixtureCatalog.nodeId.fixture36();
		page.rerenderPage();
		await waitFor(() => {
			expect(fetchAdminNodeTraffic).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture36(),
				"24h",
				expect.any(AbortSignal),
			);
			expect(screen.getByText(/Last sample:/)).toBeTruthy();
			expect(screen.queryByText("No traffic samples yet")).toBeNull();
		});

		mockRouteParams.nodeId = fixtureCatalog.nodeId.fixture32();
		page.rerenderPage();
		await waitFor(() => {
			expect(screen.getByText(/Last sample:/)).toBeTruthy();
			expect(screen.queryByText("Traffic data is partial")).toBeNull();
			expect(screen.queryByText("No traffic samples yet")).toBeNull();
		});
	});
});
