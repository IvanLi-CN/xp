import { QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

const { mockNavigate, mockReadAdminToken } = vi.hoisted(() => ({
	mockNavigate: vi.fn(),
	mockReadAdminToken: vi.fn(() => "admintoken"),
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
		}: {
			children: React.ReactNode;
			to?: string;
		}) => (
			<a href={to ?? "#"} {...rest}>
				{children}
			</a>
		),
		useNavigate: () => mockNavigate,
		useParams: () => ({ nodeId: "node-tokyo" }),
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
	return render(
		<QueryClientProvider client={queryClient}>
			<UiPrefsProvider>
				<ToastProvider>
					<NodeDetailsPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
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
		node_id: "node-tokyo",
		node_name: "Tokyo",
		api_base_url: "https://tokyo.example.com",
		access_host: "tokyo.example.com",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly" as const,
			day_of_month: 1,
			tz_offset_minutes: null,
		},
		egress_probe: {
			public_ipv4: "203.0.113.8",
			public_ipv6: "2001:db8::8",
			selected_public_ip: "203.0.113.8",
			country_code: "TW",
			geo_region: "Taiwan",
			geo_city: "Taipei",
			geo_operator: "HiNet",
			subscription_region: "taiwan" as const,
			checked_at: "2026-03-08T00:59:00Z",
			last_success_at: "2026-03-08T00:59:00Z",
			stale: false,
			error_summary: null,
		},
	};
	const refreshEgressProbe = args?.refreshEgressProbe ?? node.egress_probe;

	vi.mocked(fetchAdminNode).mockResolvedValue(node);
	vi.mocked(patchAdminNode).mockResolvedValue(node);
	vi.mocked(refreshAdminNodeEgressProbe).mockResolvedValue({
		node_id: node.node_id,
		accepted: true,
		egress_probe: refreshEgressProbe,
	});
	vi.mocked(fetchAdminNodeDeletePreview).mockResolvedValue({
		node_id: node.node_id,
		endpoints: [],
	});
	vi.mocked(deleteAdminNode).mockResolvedValue(undefined);
	vi.mocked(fetchAdminNodeRuntime).mockResolvedValue({
		node,
		summary: {
			status: "up",
			updated_at: "2026-03-08T00:59:00Z",
		},
		components: [
			{
				component: "xp",
				status: "up",
				last_ok_at: "2026-03-08T00:59:00Z",
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
				slot_start: "2026-03-08T00:30:00Z",
				status: "up",
			},
		],
		events: [
			{
				event_id: "evt-1",
				occurred_at: "2026-03-08T00:59:00Z",
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
			node_id: node.node_id,
			last_synced_at: "2026-03-08T00:59:00Z",
			last_sync_error: null,
			daily_traffic: [
				{
					date: "2026-03-08",
					uplink_bytes: 1048576,
					downlink_bytes: 2097152,
					updated_at: "2026-03-08T00:59:00Z",
				},
			],
			daily_component_status: [
				{
					date: "2026-03-08",
					components: [
						{
							component: "xray",
							status: "down",
							observed_at: "2026-03-08T00:59:00Z",
						},
					],
				},
			],
			component_status_events: [
				{
					event_id: "history-evt-1",
					occurred_at: "2026-03-08T00:55:00Z",
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
				window_start:
					window === "24h" ? "2026-03-07T01:00:00Z" : "2026-03-01T01:00:00Z",
				window_end: "2026-03-08T00:59:00Z",
				warnings: [],
				unique_ip_series: [
					{ minute: "2026-03-08T00:58:00Z", count: window === "24h" ? 1 : 3 },
					{ minute: "2026-03-08T00:59:00Z", count: window === "24h" ? 2 : 4 },
				],
				timeline: [
					{
						lane_key: "tokyo-vless::203.0.113.7",
						endpoint_id: "ep-vless",
						endpoint_tag: "tokyo-vless",
						ip: "203.0.113.7",
						minutes: window === "24h" ? 2 : 12,
						segments: [
							{
								start_minute: "2026-03-08T00:58:00Z",
								end_minute: "2026-03-08T00:59:00Z",
							},
						],
					},
				],
				ips: [
					{
						ip: "203.0.113.7",
						minutes: window === "24h" ? 2 : 12,
						endpoint_tags: ["tokyo-vless"],
						region: "Japan / Tokyo",
						operator: "ExampleNet",
						last_seen_at: "2026-03-08T00:59:00Z",
					},
				],
			},
	);
	vi.mocked(fetchAdminNodeTcpConnections).mockImplementation(
		async (_token, _nodeId, window) =>
			args?.nodeTcpConnections ?? {
				node,
				window,
				window_start:
					window === "24h" ? "2026-03-07T01:00:00Z" : "2026-03-01T01:00:00Z",
				window_end: "2026-03-08T00:59:00Z",
				warnings: [],
				endpoints: [
					{
						endpoint_id: "ep-vless",
						endpoint_tag: "tokyo-vless",
						port: 443,
					},
					{
						endpoint_id: "ep-ss",
						endpoint_tag: "tokyo-ss",
						port: 8388,
					},
				],
				per_endpoint_series: [
					{
						endpoint_id: "ep-vless",
						endpoint_tag: "tokyo-vless",
						port: 443,
						series: [
							{
								minute: "2026-03-08T00:58:00Z",
								count: window === "24h" ? 2 : 6,
							},
							{
								minute: "2026-03-08T00:59:00Z",
								count: window === "24h" ? 3 : 8,
							},
						],
					},
					{
						endpoint_id: "ep-ss",
						endpoint_tag: "tokyo-ss",
						port: 8388,
						series: [
							{
								minute: "2026-03-08T00:58:00Z",
								count: window === "24h" ? 1 : 4,
							},
							{
								minute: "2026-03-08T00:59:00Z",
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
		mockReadAdminToken.mockReturnValue("admintoken");
	});

	afterEach(() => {
		cleanup();
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
				"node-tokyo",
				"24h",
				expect.any(AbortSignal),
			);
		});

		await waitFor(() => {
			expect(screen.getAllByText("203.0.113.7").length).toBeGreaterThan(0);
		});
		expect(await screenByText("IP occupancy lanes")).toBeTruthy();

		fireEvent.click(await screenByRole("button", "7d"));
		await waitFor(() => {
			expect(fetchAdminNodeIpUsage).toHaveBeenLastCalledWith(
				"admintoken",
				"node-tokyo",
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
				"node-tokyo",
				"24h",
				expect.any(AbortSignal),
			);
		});

		expect(await screenByText("TCP connection count")).toBeTruthy();
		expect(await screenByText("tokyo-vless :443")).toBeTruthy();
		expect(await screenByText("tokyo-ss :8388")).toBeTruthy();

		fireEvent.click(await screenByRole("button", "7d"));
		await waitFor(() => {
			expect(fetchAdminNodeTcpConnections).toHaveBeenLastCalledWith(
				"admintoken",
				"node-tokyo",
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
		expect(tablist.className).toContain("w-full");

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
			expect(patchAdminNode).toHaveBeenCalledWith("admintoken", "node-tokyo", {
				quota_reset: {
					policy: "unlimited",
				},
			});
		});
		expect(
			screen.queryByText("Reset day must be an integer between 1 and 31."),
		).toBeNull();
	});

	it("shows node egress probe details and refreshes on demand", async () => {
		setupMocks({
			refreshEgressProbe: {
				public_ipv4: "198.51.100.9",
				public_ipv6: "2001:db8::9",
				selected_public_ip: "198.51.100.9",
				country_code: "US",
				geo_region: "California",
				geo_city: "San Jose",
				geo_operator: "Example Transit",
				subscription_region: "us",
				checked_at: "2026-03-08T01:05:00Z",
				last_success_at: "2026-03-08T01:05:00Z",
				stale: false,
				error_summary: null,
			},
		});
		renderPage();

		fireEvent.click(await screenByRole("tab", "Node metadata"));
		expect(await screenByText("Node egress probe")).toBeTruthy();
		await waitFor(() => {
			expect(screen.getAllByText("203.0.113.8").length).toBeGreaterThan(0);
		});
		expect(await screenByText("HiNet")).toBeTruthy();
		await waitFor(() => {
			expect(screen.getAllByText("Taiwan").length).toBeGreaterThan(0);
		});

		fireEvent.click(await screenByRole("button", "Refresh probe"));
		await waitFor(() => {
			expect(refreshAdminNodeEgressProbe).toHaveBeenCalledWith(
				"admintoken",
				"node-tokyo",
			);
		});
		await waitFor(() => {
			expect(screen.getAllByText("198.51.100.9").length).toBeGreaterThan(0);
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
			node_id: "node-tokyo",
			endpoints: [
				{
					endpoint_id: "endpoint-ss",
					tag: "tokyo-ss",
					kind: "ss2022_2022_blake3_aes_128_gcm",
					port: 8388,
				},
			],
		});
		renderPage();

		fireEvent.click(await screenByRole("tab", "Danger zone"));
		fireEvent.click(await screenByRole("button", "Delete node"));

		expect(await screenByText("Endpoints to delete: 1")).toBeTruthy();
		expect(await screenByText("tokyo-ss")).toBeTruthy();
		expect(await screenByText("SS2022")).toBeTruthy();
		fireEvent.click(await screenByRole("button", "Cancel"));

		expect(fetchAdminNodeDeletePreview).toHaveBeenCalledWith(
			"admintoken",
			"node-tokyo",
		);
		expect(deleteAdminNode).not.toHaveBeenCalled();
	});

	it("deletes node with endpoint cleanup after confirmation", async () => {
		setupMocks();
		vi.mocked(fetchAdminNodeDeletePreview).mockResolvedValueOnce({
			node_id: "node-tokyo",
			endpoints: [
				{
					endpoint_id: "endpoint-ss",
					tag: "tokyo-ss",
					kind: "ss2022_2022_blake3_aes_128_gcm",
					port: 8388,
				},
			],
		});
		renderPage();

		fireEvent.click(await screenByRole("tab", "Danger zone"));
		fireEvent.click(await screenByRole("button", "Delete node"));
		fireEvent.click(await screenByRole("button", "Delete node and endpoints"));

		await waitFor(() => {
			expect(deleteAdminNode).toHaveBeenCalledWith("admintoken", "node-tokyo", {
				deleteEndpoints: true,
				expectedEndpointIds: ["endpoint-ss"],
			});
		});
		expect(mockNavigate).toHaveBeenCalledWith({ to: "/nodes" });
	});

	it("shows online stats warning state when snapshots are unavailable", async () => {
		setupMocks({
			nodeIpUsage: {
				node: {
					node_id: "node-tokyo",
					node_name: "Tokyo",
					api_base_url: "https://tokyo.example.com",
					access_host: "tokyo.example.com",
					quota_limit_bytes: 0,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
				},
				window: "24h",
				geo_source: "country_is",
				window_start: "2026-03-07T01:00:00Z",
				window_end: "2026-03-08T00:59:00Z",
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
