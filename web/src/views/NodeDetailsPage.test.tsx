import { QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { fetchAdminNodeIpUsage } from "../api/adminIpUsage";
import { fetchAdminNodeHistory } from "../api/adminNodeHistory";
import {
	fetchAdminNodeRuntime,
	startNodeRuntimeEvents,
} from "../api/adminNodeRuntime";
import {
	deleteAdminNode,
	fetchAdminMembershipOperation,
	fetchAdminNode,
	fetchAdminNodeDeletePreview,
	fetchAdminNodeMihomoResourcePolicy,
	patchAdminNode,
	putAdminNodeMihomoResourcePolicy,
	refreshAdminNodeEgressProbe,
} from "../api/adminNodes";
import { fetchAdminRepositoryHistory } from "../api/adminRepositoryHistory";
import { fetchAdminNodeTcpConnections } from "../api/adminTcpConnections";
import { BackendApiError } from "../api/backendError";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { NodeDetailsPage } from "./NodeDetailsPage";

const { mockNavigate, mockReadAdminToken, mockRouteParams } = vi.hoisted(
	() => ({
		mockNavigate: vi.fn(),
		mockReadAdminToken: vi.fn(() => "admintoken"),
		mockRouteParams: {} as { nodeId: string },
	}),
);

vi.mock("@tanstack/react-router", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("@tanstack/react-router")>();
	return {
		...actual,
		Link: ({
			children,
			to,
			...rest
		}: {
			children: React.ReactNode;
			to?: string;
		}) => (
			<a href={to ?? "#"} {...rest}>
				{children}
			</a>
		),
		useNavigate: () => mockNavigate,
		useParams: () => mockRouteParams,
	};
});

vi.mock("../api/adminNodes");
vi.mock("../api/adminNodeRuntime");
vi.mock("../api/adminNodeHistory");
vi.mock("../api/adminRepositoryHistory");
vi.mock("../api/adminIpUsage");
vi.mock("../api/adminTcpConnections");

vi.mock("../components/auth", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../components/auth")>();
	return {
		...actual,
		readAdminToken: mockReadAdminToken,
	};
});

function renderPage() {
	const queryClient = createQueryClient();
	const page = () => (
		<QueryClientProvider client={queryClient}>
			<UiPrefsProvider>
				<ToastProvider>
					<NodeDetailsPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>
	);
	const result = render(page());
	return {
		...result,
		queryClient,
		rerenderPage: () => result.rerender(page()),
	};
}

function setupMocks(args?: {
	nodeIpUsage?: Awaited<ReturnType<typeof fetchAdminNodeIpUsage>>;
	nodeTcpConnections?: Awaited<ReturnType<typeof fetchAdminNodeTcpConnections>>;
	refreshEgressProbe?: {
		public_ipv4: string | null;
		public_ipv6: string | null;
		selected_public_ip: string | null;
		country_code: string | null;
		geo_region: string | null;
		geo_city: string | null;
		geo_operator: string | null;
		subscription_region:
			| "japan"
			| "hong_kong"
			| "taiwan"
			| "korea"
			| "singapore"
			| "us"
			| "other";
		checked_at: string;
		last_success_at: string | null;
		stale: boolean;
		error_summary: string | null;
	};
}) {
	const node = {
		node_id: fixtureCatalog.nodeId.fixture134(),
		node_name: fixtureCatalog.nodeName.fixture135(),
		api_base_url: fixtureCatalog.service.fixture136(),
		access_host: fixtureCatalog.host.fixture137(),
		quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
		quota_reset: fixtureCatalog.quota.resetNode(),
		egress_probe: {
			public_ipv4: fixtureCatalog.address.documentation192_0_2_117(),
			public_ipv6: "2001:db8::8",
			selected_public_ip: fixtureCatalog.address.documentation192_0_2_117(),
			country_code: "TW",
			geo_region: "Taiwan",
			geo_city: "Taipei",
			geo_operator: "ExampleNet",
			subscription_region: "taiwan" as const,
			checked_at: fixtureCatalog.timestamp.t20260308T005900(),
			last_success_at: fixtureCatalog.timestamp.t20260308T005900(),
			stale: false,
			error_summary: null,
		},
	};
	const refreshEgressProbe = args?.refreshEgressProbe ?? node.egress_probe;

	vi.mocked(fetchAdminNode).mockResolvedValue(node);
	vi.mocked(patchAdminNode).mockResolvedValue(node);
	vi.mocked(fetchAdminNodeMihomoResourcePolicy).mockResolvedValue({
		node_id: fixtureCatalog.nodeId.fixture134(),
		deployment_default_cidrs: [fixtureCatalog.address.privateCidr()],
		override_cidrs: null,
		effective_cidrs: [fixtureCatalog.address.privateCidr()],
		source: "deployment_default",
		status: "healthy",
		error: null,
	});
	vi.mocked(putAdminNodeMihomoResourcePolicy).mockResolvedValue({
		node_id: fixtureCatalog.nodeId.fixture134(),
		deployment_default_cidrs: [fixtureCatalog.address.privateCidr()],
		override_cidrs: [fixtureCatalog.address.privateCidr()],
		effective_cidrs: [fixtureCatalog.address.privateCidr()],
		source: "override",
		status: "healthy",
		error: null,
	});
	vi.mocked(refreshAdminNodeEgressProbe).mockResolvedValue({
		node_id: fixtureCatalog.nodeId.fixture17(),
		accepted: true,
		egress_probe: refreshEgressProbe,
	});
	vi.mocked(fetchAdminNodeDeletePreview).mockResolvedValue({
		node_id: fixtureCatalog.nodeId.fixture17(),
		endpoints: [],
	});
	vi.mocked(deleteAdminNode).mockResolvedValue({ status: "completed" });
	vi.mocked(fetchAdminNodeRuntime).mockResolvedValue({
		node,
		summary: {
			status: "up",
			updated_at: fixtureCatalog.timestamp.t20260308T005900(),
		},
		components: [
			{
				component: "xp",
				status: "up",
				last_ok_at: fixtureCatalog.timestamp.t20260308T005900(),
				last_fail_at: null,
				down_since: null,
				consecutive_failures: 0,
				recoveries_observed: 0,
				restart_attempts: 0,
				last_restart_at: null,
				last_restart_fail_at: null,
			},
		],
		recent_slots: [
			{
				slot_start: fixtureCatalog.timestamp.t20260308T003000(),
				status: "up",
			},
		],
		events: [
			{
				event_id: "evt-1",
				occurred_at: fixtureCatalog.timestamp.t20260308T005900(),
				component: "xp",
				kind: "status_changed",
				message: "xp is healthy",
				from_status: "up",
				to_status: "up",
			},
		],
	});
	vi.mocked(fetchAdminNodeHistory).mockResolvedValue({
		node,
		history: {
			node_id: fixtureCatalog.nodeId.fixture17(),
			last_synced_at: fixtureCatalog.timestamp.t20260308T005900(),
			last_sync_error: null,
			daily_traffic: [
				{
					date: "2026-03-08",
					uplink_bytes: fixtureCatalog.number.value3(),
					downlink_bytes: fixtureCatalog.number.value4(),
					updated_at: fixtureCatalog.timestamp.t20260308T005900(),
				},
			],
			daily_component_status: [
				{
					date: "2026-03-08",
					components: [
						{
							component: "xray",
							status: "down",
							observed_at: fixtureCatalog.timestamp.t20260308T005900(),
						},
					],
				},
			],
			component_status_events: [
				{
					event_id: "history-evt-1",
					occurred_at: fixtureCatalog.timestamp.t20260308T005500(),
					component: "xray",
					message: "xray status changed: up -> down",
					from_status: "up",
					to_status: "down",
				},
			],
		},
	});
	vi.mocked(fetchAdminRepositoryHistory).mockResolvedValue({
		repository: null,
		completeness: "local_only",
		coverage: null,
		watermarks: [],
		gaps: [],
		clock_skew_seconds: 0,
		records: [],
		records_truncated: false,
	});
	vi.mocked(startNodeRuntimeEvents).mockImplementation(() => ({
		close: vi.fn(),
	}));
	vi.mocked(fetchAdminNodeIpUsage).mockImplementation(
		async (_token, _nodeId, window) =>
			args?.nodeIpUsage ?? {
				node,
				window,
				geo_source: "country_is",
				window_start: fixtureCatalog.timestamp.t20260307T010000(),
				window_end: fixtureCatalog.timestamp.t20260308T005900(),
				warnings: [],
				unique_ip_series: [
					{
						minute: fixtureCatalog.timestamp.t20260308T005800(),
						count: window === "24h" ? 1 : 3,
					},
					{
						minute: fixtureCatalog.timestamp.t20260308T005900(),
						count: window === "24h" ? 2 : 4,
					},
				],
				timeline: [
					{
						lane_key: "tokyo-vless::203.0.113.7",
						endpoint_id: fixtureCatalog.endpointId.fixture138(),
						endpoint_tag: fixtureCatalog.endpointTag.fixture139(),
						ip: fixtureCatalog.address.documentation192_0_2_30(),
						minutes: window === "24h" ? 2 : 12,
						segments: [
							{
								start_minute: fixtureCatalog.timestamp.t20260308T005800(),
								end_minute: fixtureCatalog.timestamp.t20260308T005900(),
							},
						],
					},
				],
				ips: [
					{
						ip: fixtureCatalog.address.documentation192_0_2_30(),
						minutes: window === "24h" ? 2 : 12,
						endpoint_tags: [fixtureCatalog.endpointTag.fixture139()],
						region: "Japan / Tokyo",
						operator: "ExampleNet",
						last_seen_at: fixtureCatalog.timestamp.t20260308T005900(),
					},
				],
			},
	);
	vi.mocked(fetchAdminNodeTcpConnections).mockImplementation(
		async (_token, _nodeId, window) =>
			args?.nodeTcpConnections ?? {
				node,
				window,
				window_start: fixtureCatalog.timestamp.t20260307T010000(),
				window_end: fixtureCatalog.timestamp.t20260308T005900(),
				warnings: [],
				endpoints: [
					{
						endpoint_id: fixtureCatalog.endpointId.fixture138(),
						endpoint_tag: fixtureCatalog.endpointTag.fixture139(),
						port: fixtureCatalog.endpoint.port443(),
					},
					{
						endpoint_id: fixtureCatalog.endpointId.fixture140(),
						endpoint_tag: fixtureCatalog.endpointTag.fixture141(),
						port: fixtureCatalog.endpoint.port8388(),
					},
				],
				per_endpoint_series: [
					{
						endpoint_id: fixtureCatalog.endpointId.fixture138(),
						endpoint_tag: fixtureCatalog.endpointTag.fixture139(),
						port: fixtureCatalog.endpoint.port443(),
						series: [
							{
								minute: fixtureCatalog.timestamp.t20260308T005800(),
								count: window === "24h" ? 2 : 6,
							},
							{
								minute: fixtureCatalog.timestamp.t20260308T005900(),
								count: window === "24h" ? 3 : 8,
							},
						],
					},
					{
						endpoint_id: fixtureCatalog.endpointId.fixture140(),
						endpoint_tag: fixtureCatalog.endpointTag.fixture141(),
						port: fixtureCatalog.endpoint.port8388(),
						series: [
							{
								minute: fixtureCatalog.timestamp.t20260308T005800(),
								count: window === "24h" ? 1 : 4,
							},
							{
								minute: fixtureCatalog.timestamp.t20260308T005900(),
								count: window === "24h" ? 2 : 5,
							},
						],
					},
				],
			},
	);
}

describe("<NodeDetailsPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		sessionStorage.clear();
		vi.spyOn(Date, "now").mockReturnValue(Date.parse("2026-03-08T00:59:30Z"));
		mockReadAdminToken.mockReturnValue("admintoken");
		mockRouteParams.nodeId = fixtureCatalog.nodeId.fixture134();
	});

	afterEach(() => {
		cleanup();
		vi.restoreAllMocks();
	});

	it("loads IP usage on demand and switches windows", async () => {
		setupMocks();
		renderPage();

		await waitFor(() => {
			expect(fetchAdminNode).toHaveBeenCalled();
		});
		expect(fetchAdminNodeIpUsage).not.toHaveBeenCalled();

		fireEvent.click(await screenByRole("tab", "IP usage"));
		await waitFor(() => {
			expect(fetchAdminNodeIpUsage).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
				"24h",
				expect.any(AbortSignal),
			);
		});

		await waitFor(() => {
			expect(
				screen.getAllByText(fixtureCatalog.address.documentation192_0_2_30())
					.length,
			).toBeGreaterThan(0);
		});
		expect(await screenByText("IP occupancy lanes")).toBeTruthy();

		fireEvent.click(await screenByRole("button", "7d"));
		await waitFor(() => {
			expect(fetchAdminNodeIpUsage).toHaveBeenLastCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
				"7d",
				expect.any(AbortSignal),
			);
		});
	});

	it("loads TCP connections on demand and switches windows", async () => {
		setupMocks();
		renderPage();

		await waitFor(() => {
			expect(fetchAdminNode).toHaveBeenCalled();
		});
		expect(fetchAdminNodeTcpConnections).not.toHaveBeenCalled();

		fireEvent.click(await screenByRole("tab", "TCP connections"));
		await waitFor(() => {
			expect(fetchAdminNodeTcpConnections).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
				"24h",
				expect.any(AbortSignal),
			);
		});

		expect(
			await screenByText(/Per-minute ESTABLISHED inbound TCP connections/i),
		).toBeTruthy();
		expect(
			await screenByText(`${fixtureCatalog.endpointTag.fixture139()} :443`),
		).toBeTruthy();
		expect(
			await screenByText(`${fixtureCatalog.endpointTag.fixture141()} :8388`),
		).toBeTruthy();

		fireEvent.click(await screenByRole("button", "7d"));
		await waitFor(() => {
			expect(fetchAdminNodeTcpConnections).toHaveBeenLastCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
				"7d",
				expect.any(AbortSignal),
			);
		});
	});

	it("keeps the selected tab and time window when the node route changes", async () => {
		setupMocks();
		const page = renderPage();

		fireEvent.click(await screenByRole("tab", "IP usage"));
		await waitFor(() => {
			expect(fetchAdminNodeIpUsage).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
				"24h",
				expect.any(AbortSignal),
			);
		});
		fireEvent.click(await screenByRole("button", "7d"));
		await waitFor(() => {
			expect(fetchAdminNodeIpUsage).toHaveBeenLastCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
				"7d",
				expect.any(AbortSignal),
			);
		});

		mockRouteParams.nodeId = fixtureCatalog.nodeId.fixture145();
		page.rerenderPage();

		expect(await screenByRole("tab", "IP usage")).toHaveAttribute(
			"aria-selected",
			"true",
		);
		await waitFor(() => {
			expect(fetchAdminNodeIpUsage).toHaveBeenLastCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture145(),
				"7d",
				expect.any(AbortSignal),
			);
		});
	});

	it("uses a section select on mobile and keeps tabs for larger screens", async () => {
		setupMocks();
		renderPage();

		const sectionSelect = await screenByRole(
			"combobox",
			"Node details section",
		);
		expect(sectionSelect).toBeTruthy();
		expect(sectionSelect.parentElement?.className).toContain("pb-3");

		const tablist = await screenByRole("tablist", "Node details sections");
		expect(tablist.className).toContain("flex-wrap");
		expect(tablist.className).toContain("max-w-full");
		expect(tablist.classList.contains("w-full")).toBe(false);
		expect(tablist.parentElement?.parentElement?.className).toContain(
			"sm:space-y-3",
		);

		const tcpTab = await screenByRole("tab", "TCP connections");
		expect(tcpTab.className).toContain("basis-[calc(50%-0.125rem)]");
		expect(tcpTab.className).toContain("min-h-11");
	});

	it("edits Mihomo private CIDRs as tags and saves the override", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("tab", "Mihomo resources"));
		const cidrInput = await screenByRole("textbox", "Web override CIDRs");
		expect(cidrInput).toHaveAttribute("placeholder", "192.168.0.0/16");
		fireEvent.change(cidrInput, {
			target: { value: fixtureCatalog.address.privateCidr() },
		});
		fireEvent.keyDown(cidrInput, { key: "Enter" });
		await waitFor(() => {
			expect(
				cidrInput.closest('[data-testid="tag-input-control"]'),
			).toHaveTextContent(fixtureCatalog.address.privateCidr());
		});

		fireEvent.click(await screenByRole("button", "Save override"));
		await waitFor(() => {
			expect(putAdminNodeMihomoResourcePolicy).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
				[fixtureCatalog.address.privateCidr()],
			);
		});

		for (const name of [
			"Refresh",
			"Disable private targets",
			"Save override",
			"Restore deployment default",
		]) {
			const button = await screenByRole("button", name);
			expect(button.className).toContain("h-10");
			expect(button.className).toContain("min-h-11");
		}
		expect(
			(await screenByRole("button", "Disable private targets")).className,
		).toContain("border");
	});

	it("allows saving unlimited quota reset after entering an invalid monthly day", async () => {
		setupMocks();
		renderPage();

		fireEvent.click(await screenByRole("tab", "Quota reset"));
		fireEvent.change(await screenByLabel("Day of month"), {
			target: { value: "99" },
		});
		fireEvent.click(await screenByLabel("Policy"));
		fireEvent.click(await screenByRole("option", "unlimited"));
		fireEvent.click(await screenByRole("button", "Save changes"));

		await waitFor(() => {
			expect(patchAdminNode).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
				{
					quota_reset: fixtureCatalog.quota.resetUnlimitedRequest(),
				},
			);
		});
		expect(
			screen.queryByText("Reset day must be an integer between 1 and 31."),
		).toBeNull();
	});

	it("shows node egress probe details and refreshes on demand", async () => {
		setupMocks({
			refreshEgressProbe: {
				public_ipv4: fixtureCatalog.address.documentation192_0_2_143(),
				public_ipv6: "2001:db8::9",
				selected_public_ip: fixtureCatalog.address.documentation192_0_2_143(),
				country_code: "US",
				geo_region: "California",
				geo_city: "San Jose",
				geo_operator: "Example Transit",
				subscription_region: "us",
				checked_at: fixtureCatalog.timestamp.t20260308T010500(),
				last_success_at: fixtureCatalog.timestamp.t20260308T010500(),
				stale: false,
				error_summary: null,
			},
		});
		renderPage();

		fireEvent.click(await screenByRole("tab", "Node metadata"));
		expect(await screenByText("Node egress probe")).toBeTruthy();
		await waitFor(() => {
			expect(
				screen.getAllByText(fixtureCatalog.address.documentation192_0_2_117())
					.length,
			).toBeGreaterThan(0);
		});
		expect(await screenByText("ExampleNet")).toBeTruthy();
		await waitFor(() => {
			expect(screen.getAllByText("Taiwan").length).toBeGreaterThan(0);
		});

		fireEvent.click(await screenByRole("button", "Refresh probe"));
		await waitFor(() => {
			expect(refreshAdminNodeEgressProbe).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
			);
		});
		await waitFor(() => {
			expect(
				screen.getAllByText(fixtureCatalog.address.documentation192_0_2_143())
					.length,
			).toBeGreaterThan(0);
		});
		expect(await screenByText("Example Transit")).toBeTruthy();
		await waitFor(() => {
			expect(screen.getAllByText("US").length).toBeGreaterThan(0);
		});
		expect(fetchAdminNode).toHaveBeenCalledTimes(1);
	});

	it("previews endpoint cleanup before deleting a node", async () => {
		setupMocks();
		vi.mocked(fetchAdminNodeDeletePreview).mockResolvedValueOnce({
			node_id: fixtureCatalog.nodeId.fixture134(),
			endpoints: [
				{
					endpoint_id: fixtureCatalog.endpointId.fixture144(),
					tag: fixtureCatalog.endpointTag.fixture141(),
					kind: fixtureCatalog.endpoint.ssKind(),
					port: fixtureCatalog.endpoint.port8388(),
				},
			],
		});
		renderPage();

		fireEvent.click(await screenByRole("tab", "Danger zone"));
		fireEvent.click(await screenByRole("button", "Delete node"));

		expect(await screenByText("Endpoints to delete: 1")).toBeTruthy();
		expect(
			await screenByText(fixtureCatalog.endpointTag.fixture141()),
		).toBeTruthy();
		expect(await screenByText("SS2022")).toBeTruthy();
		fireEvent.click(await screenByRole("button", "Cancel"));

		expect(fetchAdminNodeDeletePreview).toHaveBeenCalledWith(
			"admintoken",
			fixtureCatalog.nodeId.fixture134(),
		);
		expect(deleteAdminNode).not.toHaveBeenCalled();
	});

	it("deletes node with endpoint cleanup after confirmation", async () => {
		setupMocks();
		vi.mocked(fetchAdminNodeDeletePreview).mockResolvedValueOnce({
			node_id: fixtureCatalog.nodeId.fixture134(),
			endpoints: [
				{
					endpoint_id: fixtureCatalog.endpointId.fixture144(),
					tag: fixtureCatalog.endpointTag.fixture141(),
					kind: fixtureCatalog.endpoint.ssKind(),
					port: fixtureCatalog.endpoint.port8388(),
				},
			],
		});
		const { queryClient } = renderPage();
		queryClient.setQueryData(["adminNodes", "admintoken"], {
			items: [{ node_id: fixtureCatalog.nodeId.fixture134() }],
		});
		queryClient.setQueryData(["adminEndpoints", "admintoken"], {
			items: [
				{
					endpoint_id: fixtureCatalog.endpointId.fixture144(),
					node_id: fixtureCatalog.nodeId.fixture134(),
				},
			],
		});

		fireEvent.click(await screenByRole("tab", "Danger zone"));
		fireEvent.click(await screenByRole("button", "Delete node"));
		fireEvent.click(await screenByRole("button", "Delete node and endpoints"));

		await waitFor(() => {
			expect(deleteAdminNode).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.nodeId.fixture134(),
				{
					deleteEndpoints: true,
					expectedEndpointIds: [fixtureCatalog.endpointId.fixture144()],
				},
			);
		});
		expect(
			queryClient.getQueryData<{ items: Array<{ node_id: string }> }>([
				"adminNodes",
				"admintoken",
			])?.items,
		).toEqual([]);
		expect(
			queryClient.getQueryData<{ items: Array<{ endpoint_id: string }> }>([
				"adminEndpoints",
				"admintoken",
			])?.items,
		).toEqual([]);
		expect(mockNavigate).toHaveBeenCalledWith({ to: "/nodes" });
	});

	it("keeps a 202 node deletion operation visible and disables duplicate deletion", async () => {
		setupMocks();
		const operationId = "e9a42d3c-9812-4a38-8a23-2d8cb7770001";
		vi.mocked(deleteAdminNode).mockResolvedValueOnce({
			status: "accepted",
			operationId,
			phase: "prepared",
		});
		vi.mocked(fetchAdminMembershipOperation).mockResolvedValue({
			operation_id: operationId,
			kind: "remove_node",
			raft_node_id: 42,
			node_id: fixtureCatalog.nodeId.fixture134(),
			expected_membership: "membership-revision",
			phase: "prepared",
			delete_endpoints: false,
			expected_endpoint_ids: [],
			created_at: fixtureCatalog.timestamp.t20260308T005900(),
			next_retry_at: null,
			terminal_at: null,
			evidence: "admin node deletion accepted",
		});
		renderPage();

		fireEvent.click(await screenByRole("tab", "Danger zone"));
		fireEvent.click(await screenByRole("button", "Delete node"));
		fireEvent.click(await screenByRole("button", "Delete"));

		await waitFor(() => {
			expect(fetchAdminMembershipOperation).toHaveBeenCalledWith(
				"admintoken",
				operationId,
				expect.any(AbortSignal),
			);
		});
		expect(screen.getByRole("status")).toHaveTextContent(
			"Node deletion is continuing.",
		);
		expect(await screenByRole("button", "Delete node")).toBeDisabled();
	});

	it("clears an expired stored node deletion operation after a 404", async () => {
		setupMocks();
		const operationId = "e9a42d3c-9812-4a38-8a23-2d8cb7770001";
		sessionStorage.setItem(
			`xp_node_delete_operation_v1:${fixtureCatalog.nodeId.fixture134()}`,
			operationId,
		);
		vi.mocked(fetchAdminMembershipOperation).mockRejectedValue(
			new BackendApiError({ status: 404, message: "operation not found" }),
		);
		renderPage();

		await waitFor(() => {
			expect(
				sessionStorage.getItem(
					`xp_node_delete_operation_v1:${fixtureCatalog.nodeId.fixture134()}`,
				),
			).toBeNull();
		});
		fireEvent.click(await screenByRole("tab", "Danger zone"));
		expect(await screenByRole("button", "Delete node")).toBeEnabled();
	});

	it("shows online stats warning state when snapshots are unavailable", async () => {
		setupMocks({
			nodeIpUsage: {
				node: {
					node_id: fixtureCatalog.nodeId.fixture134(),
					node_name: fixtureCatalog.nodeName.fixture135(),
					api_base_url: fixtureCatalog.service.fixture136(),
					access_host: fixtureCatalog.host.fixture137(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.resetNode(),
				},
				window: "24h",
				geo_source: "country_is",
				window_start: fixtureCatalog.timestamp.t20260307T010000(),
				window_end: fixtureCatalog.timestamp.t20260308T005900(),
				warnings: [
					{
						code: "online_stats_unavailable",
						message:
							"Xray online IP stats are unavailable; enable statsUserOnline to collect inbound IP usage.",
					},
				],
				unique_ip_series: [],
				timeline: [],
				ips: [],
			},
		});
		renderPage();

		fireEvent.click(await screenByRole("tab", "IP usage"));
		expect(await screenByText("Online snapshots are unavailable")).toBeTruthy();
	});

	it("shows mirrored history when live runtime is unavailable", async () => {
		setupMocks();
		vi.mocked(fetchAdminNodeRuntime).mockRejectedValue(
			new Error("request timeout"),
		);
		renderPage();

		expect(await screenByText(/Live node API is unavailable/)).toBeTruthy();
		expect(await screenByText("Daily traffic mirror")).toBeTruthy();
		expect(await screenByText("Daily component snapshot")).toBeTruthy();
		expect(await screenByText("xray status changed: up -> down")).toBeTruthy();
	});
});

async function screenByRole(role: string, name: string): Promise<HTMLElement> {
	const { findByRole } = await import("@testing-library/react");
	return findByRole(document.body, role as never, { name });
}

async function screenByLabel(label: string): Promise<HTMLElement> {
	const { findByLabelText } = await import("@testing-library/react");
	return findByLabelText(document.body, label);
}

async function screenByText(text: string | RegExp): Promise<HTMLElement> {
	const { findByText } = await import("@testing-library/react");
	return findByText(document.body, text);
}
