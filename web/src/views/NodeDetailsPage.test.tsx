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
	fetchAdminNode,
	fetchAdminNodeDeletePreview,
	patchAdminNode,
	refreshAdminNodeEgressProbe,
} from "../api/adminNodes";
import { fetchAdminNodeTcpConnections } from "../api/adminTcpConnections";
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
		node_id: fixtureCatalog.slotString.s134(),
		node_name: fixtureCatalog.slotString.s135(),
		api_base_url: fixtureCatalog.slotString.s136(),
		access_host: fixtureCatalog.slotString.s137(),
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly" as const,
			day_of_month: 1,
			tz_offset_minutes: null,
		},
		egress_probe: {
			public_ipv4: fixtureCatalog.slotString.s116(),
			public_ipv6: "2001:db8::8",
			selected_public_ip: fixtureCatalog.slotString.s116(),
			country_code: "TW",
			geo_region: "Taiwan",
			geo_city: "Taipei",
			geo_operator: "ExampleNet",
			subscription_region: "taiwan" as const,
			checked_at: fixtureCatalog.slotString.s48(),
			last_success_at: fixtureCatalog.slotString.s48(),
			stale: false,
			error_summary: null,
		},
	};
	const refreshEgressProbe = args?.refreshEgressProbe ?? node.egress_probe;

	vi.mocked(fetchAdminNode).mockResolvedValue(node);
	vi.mocked(patchAdminNode).mockResolvedValue(node);
	vi.mocked(refreshAdminNodeEgressProbe).mockResolvedValue({
		node_id: fixtureCatalog.slotString.s17(),
		accepted: true,
		egress_probe: refreshEgressProbe,
	});
	vi.mocked(fetchAdminNodeDeletePreview).mockResolvedValue({
		node_id: fixtureCatalog.slotString.s17(),
		endpoints: [],
	});
	vi.mocked(deleteAdminNode).mockResolvedValue(undefined);
	vi.mocked(fetchAdminNodeRuntime).mockResolvedValue({
		node,
		summary: {
			status: "up",
			updated_at: fixtureCatalog.slotString.s48(),
		},
		components: [
			{
				component: "xp",
				status: "up",
				last_ok_at: fixtureCatalog.slotString.s48(),
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
				slot_start: fixtureCatalog.slotString.s655(),
				status: "up",
			},
		],
		events: [
			{
				event_id: "evt-1",
				occurred_at: fixtureCatalog.slotString.s48(),
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
			node_id: fixtureCatalog.slotString.s17(),
			last_synced_at: fixtureCatalog.slotString.s48(),
			last_sync_error: null,
			daily_traffic: [
				{
					date: "2026-03-08",
					uplink_bytes: fixtureCatalog.slotNumber.n2(),
					downlink_bytes: fixtureCatalog.slotNumber.n3(),
					updated_at: fixtureCatalog.slotString.s48(),
				},
			],
			daily_component_status: [
				{
					date: "2026-03-08",
					components: [
						{
							component: "xray",
							status: "down",
							observed_at: fixtureCatalog.slotString.s48(),
						},
					],
				},
			],
			component_status_events: [
				{
					event_id: "history-evt-1",
					occurred_at: fixtureCatalog.slotString.s656(),
					component: "xray",
					message: "xray status changed: up -> down",
					from_status: "up",
					to_status: "down",
				},
			],
		},
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
				window_start: fixtureCatalog.slotString.s47(),
				window_end: fixtureCatalog.slotString.s48(),
				warnings: [],
				unique_ip_series: [
					{
						minute: fixtureCatalog.slotString.s52(),
						count: window === "24h" ? 1 : 3,
					},
					{
						minute: fixtureCatalog.slotString.s48(),
						count: window === "24h" ? 2 : 4,
					},
				],
				timeline: [
					{
						lane_key: "tokyo-vless::203.0.113.7",
						endpoint_id: fixtureCatalog.slotString.s138(),
						endpoint_tag: fixtureCatalog.slotString.s139(),
						ip: fixtureCatalog.slotString.s29(),
						minutes: window === "24h" ? 2 : 12,
						segments: [
							{
								start_minute: fixtureCatalog.slotString.s52(),
								end_minute: fixtureCatalog.slotString.s48(),
							},
						],
					},
				],
				ips: [
					{
						ip: fixtureCatalog.slotString.s29(),
						minutes: window === "24h" ? 2 : 12,
						endpoint_tags: [fixtureCatalog.slotString.s139()],
						region: "Japan / Tokyo",
						operator: "ExampleNet",
						last_seen_at: fixtureCatalog.slotString.s48(),
					},
				],
			},
	);
	vi.mocked(fetchAdminNodeTcpConnections).mockImplementation(
		async (_token, _nodeId, window) =>
			args?.nodeTcpConnections ?? {
				node,
				window,
				window_start: fixtureCatalog.slotString.s47(),
				window_end: fixtureCatalog.slotString.s48(),
				warnings: [],
				endpoints: [
					{
						endpoint_id: fixtureCatalog.slotString.s138(),
						endpoint_tag: fixtureCatalog.slotString.s139(),
						port: 443,
					},
					{
						endpoint_id: fixtureCatalog.slotString.s140(),
						endpoint_tag: fixtureCatalog.slotString.s141(),
						port: 8388,
					},
				],
				per_endpoint_series: [
					{
						endpoint_id: fixtureCatalog.slotString.s138(),
						endpoint_tag: fixtureCatalog.slotString.s139(),
						port: 443,
						series: [
							{
								minute: fixtureCatalog.slotString.s52(),
								count: window === "24h" ? 2 : 6,
							},
							{
								minute: fixtureCatalog.slotString.s48(),
								count: window === "24h" ? 3 : 8,
							},
						],
					},
					{
						endpoint_id: fixtureCatalog.slotString.s140(),
						endpoint_tag: fixtureCatalog.slotString.s141(),
						port: 8388,
						series: [
							{
								minute: fixtureCatalog.slotString.s52(),
								count: window === "24h" ? 1 : 4,
							},
							{
								minute: fixtureCatalog.slotString.s48(),
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
		vi.spyOn(Date, "now").mockReturnValue(Date.parse("2026-03-08T00:59:30Z"));
		mockReadAdminToken.mockReturnValue("admintoken");
		mockRouteParams.nodeId = fixtureCatalog.slotString.s134();
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
				fixtureCatalog.slotString.s134(),
				"24h",
				expect.any(AbortSignal),
			);
		});

		await waitFor(() => {
			expect(
				screen.getAllByText(fixtureCatalog.slotString.s29()).length,
			).toBeGreaterThan(0);
		});
		expect(await screenByText("IP occupancy lanes")).toBeTruthy();

		fireEvent.click(await screenByRole("button", "7d"));
		await waitFor(() => {
			expect(fetchAdminNodeIpUsage).toHaveBeenLastCalledWith(
				"admintoken",
				fixtureCatalog.slotString.s134(),
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
				fixtureCatalog.slotString.s134(),
				"24h",
				expect.any(AbortSignal),
			);
		});

		expect(await screenByText("TCP connection count")).toBeTruthy();
		expect(
			await screenByText(`${fixtureCatalog.slotString.s139()} :443`),
		).toBeTruthy();
		expect(
			await screenByText(`${fixtureCatalog.slotString.s141()} :8388`),
		).toBeTruthy();

		fireEvent.click(await screenByRole("button", "7d"));
		await waitFor(() => {
			expect(fetchAdminNodeTcpConnections).toHaveBeenLastCalledWith(
				"admintoken",
				fixtureCatalog.slotString.s134(),
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
				fixtureCatalog.slotString.s134(),
				"24h",
				expect.any(AbortSignal),
			);
		});
		fireEvent.click(await screenByRole("button", "7d"));
		await waitFor(() => {
			expect(fetchAdminNodeIpUsage).toHaveBeenLastCalledWith(
				"admintoken",
				fixtureCatalog.slotString.s134(),
				"7d",
				expect.any(AbortSignal),
			);
		});

		mockRouteParams.nodeId = fixtureCatalog.slotString.s145();
		page.rerenderPage();

		expect(await screenByRole("tab", "IP usage")).toHaveAttribute(
			"aria-selected",
			"true",
		);
		await waitFor(() => {
			expect(fetchAdminNodeIpUsage).toHaveBeenLastCalledWith(
				"admintoken",
				fixtureCatalog.slotString.s145(),
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
				fixtureCatalog.slotString.s134(),
				{
					quota_reset: {
						policy: "unlimited",
					},
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
				public_ipv4: fixtureCatalog.slotString.s142(),
				public_ipv6: "2001:db8::9",
				selected_public_ip: fixtureCatalog.slotString.s142(),
				country_code: "US",
				geo_region: "California",
				geo_city: "San Jose",
				geo_operator: "Example Transit",
				subscription_region: "us",
				checked_at: fixtureCatalog.slotString.s143(),
				last_success_at: fixtureCatalog.slotString.s143(),
				stale: false,
				error_summary: null,
			},
		});
		renderPage();

		fireEvent.click(await screenByRole("tab", "Node metadata"));
		expect(await screenByText("Node egress probe")).toBeTruthy();
		await waitFor(() => {
			expect(
				screen.getAllByText(fixtureCatalog.slotString.s116()).length,
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
				fixtureCatalog.slotString.s134(),
			);
		});
		await waitFor(() => {
			expect(
				screen.getAllByText(fixtureCatalog.slotString.s142()).length,
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
			node_id: fixtureCatalog.slotString.s134(),
			endpoints: [
				{
					endpoint_id: fixtureCatalog.slotString.s144(),
					tag: fixtureCatalog.slotString.s141(),
					kind: fixtureCatalog.endpoint.ssKind(),
					port: 8388,
				},
			],
		});
		renderPage();

		fireEvent.click(await screenByRole("tab", "Danger zone"));
		fireEvent.click(await screenByRole("button", "Delete node"));

		expect(await screenByText("Endpoints to delete: 1")).toBeTruthy();
		expect(await screenByText(fixtureCatalog.slotString.s141())).toBeTruthy();
		expect(await screenByText("SS2022")).toBeTruthy();
		fireEvent.click(await screenByRole("button", "Cancel"));

		expect(fetchAdminNodeDeletePreview).toHaveBeenCalledWith(
			"admintoken",
			fixtureCatalog.slotString.s134(),
		);
		expect(deleteAdminNode).not.toHaveBeenCalled();
	});

	it("deletes node with endpoint cleanup after confirmation", async () => {
		setupMocks();
		vi.mocked(fetchAdminNodeDeletePreview).mockResolvedValueOnce({
			node_id: fixtureCatalog.slotString.s134(),
			endpoints: [
				{
					endpoint_id: fixtureCatalog.slotString.s144(),
					tag: fixtureCatalog.slotString.s141(),
					kind: fixtureCatalog.endpoint.ssKind(),
					port: 8388,
				},
			],
		});
		const { queryClient } = renderPage();
		queryClient.setQueryData(["adminNodes", "admintoken"], {
			items: [{ node_id: fixtureCatalog.slotString.s134() }],
		});
		queryClient.setQueryData(["adminEndpoints", "admintoken"], {
			items: [
				{
					endpoint_id: fixtureCatalog.slotString.s144(),
					node_id: fixtureCatalog.slotString.s134(),
				},
			],
		});

		fireEvent.click(await screenByRole("tab", "Danger zone"));
		fireEvent.click(await screenByRole("button", "Delete node"));
		fireEvent.click(await screenByRole("button", "Delete node and endpoints"));

		await waitFor(() => {
			expect(deleteAdminNode).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.slotString.s134(),
				{
					deleteEndpoints: true,
					expectedEndpointIds: [fixtureCatalog.slotString.s144()],
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

	it("shows online stats warning state when snapshots are unavailable", async () => {
		setupMocks({
			nodeIpUsage: {
				node: {
					node_id: fixtureCatalog.slotString.s134(),
					node_name: fixtureCatalog.slotString.s135(),
					api_base_url: fixtureCatalog.slotString.s136(),
					access_host: fixtureCatalog.slotString.s137(),
					quota_limit_bytes: 0,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
				},
				window: "24h",
				geo_source: "country_is",
				window_start: fixtureCatalog.slotString.s47(),
				window_end: fixtureCatalog.slotString.s48(),
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
