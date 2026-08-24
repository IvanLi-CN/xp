import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { fetchAdminHistoryRepositories } from "../api/adminHistoryRepositories";
import { createAdminJoinToken } from "../api/adminJoinTokens";
import { fetchAdminNodesRuntime } from "../api/adminNodeRuntime";
import { BackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { NodesPage } from "./NodesPage";

vi.mock("@tanstack/react-router", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("@tanstack/react-router")>();
	return {
		...actual,
		useLocation: () => ({ pathname: window.location.pathname }),
		useNavigate:
			() =>
			({ to }: { to: string }) => {
				window.history.pushState({}, "", to);
			},
		Link: ({
			children,
			to,
			params,
			search,
			...rest
		}: {
			children: React.ReactNode;
			to?: string;
			params?: Record<string, string>;
			search?: { redirect?: string };
		}) => {
			let href = to ?? "#";
			if (params) {
				for (const [key, value] of Object.entries(params)) {
					href = href.replace(`$${key}`, value);
				}
			}
			if (search?.redirect) {
				href += `?redirect=${encodeURIComponent(search.redirect)}`;
			}
			return (
				<a href={href} {...rest}>
					{children}
				</a>
			);
		},
	};
});

vi.mock("../api/adminJoinTokens");
vi.mock("../api/adminHistoryRepositories");
vi.mock("../api/adminNodeRuntime");
vi.mock("../api/clusterInfo");
vi.mock("../offline/useQueryWithOfflineFallback", () => ({
	useQueryWithOfflineFallback: (
		_queryKey: readonly unknown[],
		query: unknown,
	) => query,
}));
vi.mock("../components/Icon", () => ({
	Icon: ({ ariaLabel }: { ariaLabel?: string }) => (
		<span aria-hidden={ariaLabel ? undefined : "true"} aria-label={ariaLabel} />
	),
}));

vi.mock("../components/auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../components/auth")>();
	return {
		...actual,
		readAdminToken: () => "admintoken",
	};
});

function renderPage() {
	const queryClient = createQueryClient();
	return render(
		<QueryClientProvider client={queryClient}>
			<UiPrefsProvider>
				<ToastProvider>
					<NodesPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

describe("<NodesPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		window.history.pushState({}, fixtureCatalog.host.fixture99(), "/");
		globalThis.ResizeObserver = class {
			observe() {
				// no-op for jsdom layout tests
			}
			disconnect() {
				// no-op for jsdom layout tests
			}
			unobserve() {
				// no-op for jsdom layout tests
			}
		} as typeof ResizeObserver;

		vi.mocked(fetchClusterInfo).mockResolvedValue({
			cluster_id: fixtureCatalog.cluster.fixture84(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			role: "leader",
			leader_api_base_url: fixtureCatalog.service.fixture178(),
			term: 12,
			xp_version: "1.0.0",
		});
		vi.mocked(createAdminJoinToken).mockResolvedValue({
			join_token: "join-token-1",
		});
		vi.mocked(fetchAdminHistoryRepositories).mockResolvedValue({
			configured: false,
			partial: false,
			unreachable_node_ids: [],
			items: [],
		});
		vi.mocked(fetchAdminNodesRuntime).mockResolvedValue({
			partial: false,
			unreachable_nodes: [],
			items: [
				{
					node_id: fixtureCatalog.nodeId.fixture32(),
					node_name: fixtureCatalog.nodeName.fixture33(),
					api_base_url: fixtureCatalog.service.fixture178(),
					access_host: fixtureCatalog.host.fixture179(),
					summary: {
						status: "up",
						updated_at: fixtureCatalog.timestamp.t20260301T000000(),
					},
					components: [
						{
							component: "xp",
							status: "up",
							consecutive_failures: 0,
							recoveries_observed: 2,
							restart_attempts: 0,
						},
					],
					recent_slots: [
						{
							slot_start: fixtureCatalog.timestamp.t20260301T000000(),
							status: "up",
						},
					],
				},
				{
					node_id: fixtureCatalog.nodeId.fixture36(),
					node_name: fixtureCatalog.host.fixture99(),
					api_base_url: fixtureCatalog.service.fixture180(),
					access_host: fixtureCatalog.host.fixture181(),
					summary: {
						status: "degraded",
						updated_at: fixtureCatalog.timestamp.t20260301T000000(),
					},
					components: [
						{
							component: "xray",
							status: "down",
							consecutive_failures: 3,
							recoveries_observed: 1,
							restart_attempts: 1,
						},
					],
					recent_slots: [
						{
							slot_start: fixtureCatalog.timestamp.t20260301T000000(),
							status: "degraded",
						},
					],
				},
			],
		});
	});

	it("renders dual node actions in the shared inventory list", async () => {
		renderPage();

		await waitFor(() => {
			expect(fetchAdminNodesRuntime).toHaveBeenCalledWith(
				"admintoken",
				expect.any(AbortSignal),
			);
		});

		const detailsLinks = await screen.findAllByRole("link", {
			name: "Details",
		});
		expect(detailsLinks.map((link) => link.getAttribute("href"))).toEqual([
			`/nodes/${fixtureCatalog.nodeId.fixture32()}`,
			`/nodes/${fixtureCatalog.nodeId.fixture36()}`,
		]);
		const openOnNodeLinks = screen.getAllByRole("link", {
			name: "Open on node",
		});
		expect(openOnNodeLinks.map((link) => link.getAttribute("href"))).toEqual([
			`${fixtureCatalog.service.fixture178()}/?login_token=admintoken`,
			`${fixtureCatalog.service.fixture180()}/?login_token=admintoken`,
		]);
	});

	it.each([
		["/nodes", "节点"],
		["/nodes/join", "加入节点"],
		["/nodes/repositories", "历史仓库"],
	] as const)("restores the active module tab from %s", async (path, label) => {
		window.history.pushState({}, fixtureCatalog.host.fixture99(), path);
		renderPage();

		expect(screen.getByRole("tab", { name: label })).toHaveAttribute(
			"aria-selected",
			"true",
		);
	});

	it("does not repeat the node inventory title below the page heading", () => {
		renderPage();

		expect(
			screen.queryByRole("heading", { name: "Node inventory" }),
		).not.toBeInTheDocument();
	});

	it("keeps the join token state mounted while switching module paths", async () => {
		window.history.pushState(
			{},
			fixtureCatalog.host.fixture99(),
			"/nodes/join",
		);
		renderPage();

		await userEvent.click(screen.getByRole("button", { name: "Create token" }));
		await screen.findByText("join-token-1");
		await userEvent.click(screen.getByRole("tab", { name: "历史仓库" }));

		expect(window.location.pathname).toBe("/nodes/repositories");
		expect(screen.getByText("join-token-1")).toBeInTheDocument();
	});

	it("offers sign-in recovery for an unauthorized nodes request", async () => {
		window.history.pushState(
			{},
			fixtureCatalog.host.fixture99(),
			"/nodes?view=table#history",
		);
		vi.mocked(fetchAdminNodesRuntime).mockRejectedValue(
			new BackendApiError({
				status: 401,
				code: "unauthorized",
				message: "missing or invalid authorization token",
			}),
		);

		renderPage();

		const signInLinks = await screen.findAllByRole("link", {
			name: "Sign in",
		});
		expect(signInLinks).toHaveLength(2);
		for (const signInLink of signInLinks) {
			expect(signInLink).toHaveAttribute(
				"href",
				"/login?redirect=%2Fnodes%3Fview%3Dtable%23history",
			);
		}
		expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
	});

	it("keeps cached inventory visible after a refresh returns 401", async () => {
		renderPage();

		await screen.findByText(fixtureCatalog.nodeName.fixture33());
		vi.mocked(fetchAdminNodesRuntime).mockRejectedValueOnce(
			new BackendApiError({
				status: 401,
				code: "unauthorized",
				message: "missing or invalid authorization token",
			}),
		);
		await userEvent.click(screen.getByRole("button", { name: "Refresh" }));

		await screen.findByText("Showing cached node inventory");
		expect(screen.getByText(fixtureCatalog.nodeName.fixture33())).toBeVisible();
		expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute(
			"href",
			"/login?redirect=%2F",
		);
	});

	it("keeps forbidden errors on the normal retry path", async () => {
		vi.mocked(fetchAdminNodesRuntime).mockRejectedValue(
			new BackendApiError({
				status: 403,
				code: "forbidden",
				message: "access denied",
			}),
		);

		renderPage();

		await screen.findByRole("button", { name: "Retry" });
		expect(
			screen.queryByRole("link", { name: "Sign in" }),
		).not.toBeInTheDocument();
	});
});
