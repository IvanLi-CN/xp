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
import {
	fetchAdminNodeRuntime,
	startNodeRuntimeEvents,
} from "../api/adminNodeRuntime";
import {
	deleteAdminNode,
	fetchAdminNode,
	patchAdminNode,
} from "../api/adminNodes";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { NodeDetailsPage } from "./NodeDetailsPage";

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
		useNavigate: () => vi.fn(),
		useParams: () => ({ nodeId: "node-tokyo" }),
	};
});

vi.mock("../api/adminNodes");
vi.mock("../api/adminNodeRuntime");
vi.mock("../api/adminIpUsage");

const { mockReadAdminToken } = vi.hoisted(() => ({
	mockReadAdminToken: vi.fn(() => "admintoken"),
}));

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
	};

	vi.mocked(fetchAdminNode).mockResolvedValue(node);
	vi.mocked(patchAdminNode).mockResolvedValue(node);
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
	vi.mocked(startNodeRuntimeEvents).mockImplementation(() => ({
		close: vi.fn(),
	}));
	vi.mocked(fetchAdminNodeIpUsage).mockImplementation(
		async (_token, _nodeId, window) =>
			args?.nodeIpUsage ?? {
				node,
				window,
				geo_source: "managed_dbip_lite",
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
				geo_source: "missing",
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
});

async function screenByRole(role: string, name: string): Promise<HTMLElement> {
	const { findByRole } = await import("@testing-library/react");
	return findByRole(document.body, role as never, { name });
}

async function screenByText(text: string): Promise<HTMLElement> {
	const { findByText } = await import("@testing-library/react");
	return findByText(document.body, text);
}
