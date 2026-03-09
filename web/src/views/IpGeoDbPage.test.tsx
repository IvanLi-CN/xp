import { QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
	type AdminIpGeoDbResponse,
	fetchAdminIpGeoDb,
	patchAdminIpGeoDb,
	triggerAdminIpGeoDbUpdate,
} from "../api/adminIpGeoDb";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { IpGeoDbPage } from "./IpGeoDbPage";

vi.mock("../api/adminIpGeoDb");

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

function buildResponse(
	overrides?: Partial<AdminIpGeoDbResponse>,
): AdminIpGeoDbResponse {
	return {
		settings: {
			provider: "dbip_lite",
			auto_update_enabled: false,
			update_interval_days: 1,
		},
		partial: false,
		unreachable_nodes: [],
		nodes: [
			{
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
				mode: "managed",
				running: false,
				city_db_path: "/var/lib/xp/geoip/dbip-city-lite.mmdb",
				asn_db_path: "/var/lib/xp/geoip/dbip-asn-lite.mmdb",
				last_started_at: null,
				last_success_at: "2026-03-08T00:00:00Z",
				next_scheduled_at: "2026-03-09T00:00:00Z",
				last_error: null,
			},
			{
				node: {
					node_id: "node-osaka",
					node_name: "Osaka",
					api_base_url: "https://osaka.example.com",
					access_host: "osaka.example.com",
					quota_limit_bytes: 0,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
				},
				mode: "external_override",
				running: false,
				city_db_path: "/etc/xp/custom-city.mmdb",
				asn_db_path: "/etc/xp/custom-asn.mmdb",
				last_started_at: null,
				last_success_at: null,
				next_scheduled_at: null,
				last_error:
					"Managed downloader is skipped because env override is present.",
			},
		],
		...overrides,
	};
}

function renderPage() {
	const queryClient = createQueryClient();
	return render(
		<QueryClientProvider client={queryClient}>
			<UiPrefsProvider>
				<ToastProvider>
					<IpGeoDbPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

describe("<IpGeoDbPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		mockReadAdminToken.mockReturnValue("admintoken");
	});

	afterEach(() => {
		cleanup();
		vi.useRealTimers();
	});

	it("renders cluster settings and node runtime rows", async () => {
		vi.mocked(fetchAdminIpGeoDb).mockResolvedValue(buildResponse());
		renderPage();

		expect(
			await screen.findByRole("heading", { name: "IP geolocation" }),
		).toBeInTheDocument();
		expect(await screen.findByText("Update policy")).toBeInTheDocument();
		expect(screen.getByText("Provider: DB-IP Lite")).toBeInTheDocument();
		expect(screen.getByText("Node runtime")).toBeInTheDocument();
		expect(screen.getByText("Tokyo")).toBeInTheDocument();
		expect(screen.getByText("External override")).toBeInTheDocument();
	});

	it("saves update settings", async () => {
		vi.mocked(fetchAdminIpGeoDb).mockResolvedValue(buildResponse());
		vi.mocked(patchAdminIpGeoDb).mockResolvedValue({
			provider: "dbip_lite",
			auto_update_enabled: true,
			update_interval_days: 3,
		});
		renderPage();

		await screen.findByText("Update policy");
		fireEvent.click(
			screen.getByRole("checkbox", { name: /automatic updates/i }),
		);
		fireEvent.change(
			screen.getByRole("spinbutton", { name: /update interval/i }),
			{
				target: { value: "3" },
			},
		);
		fireEvent.click(screen.getByRole("button", { name: "Save settings" }));

		await waitFor(() => {
			expect(patchAdminIpGeoDb).toHaveBeenCalledWith("admintoken", {
				auto_update_enabled: true,
				update_interval_days: 3,
			});
		});
	});

	it("triggers manual updates", async () => {
		vi.mocked(fetchAdminIpGeoDb).mockResolvedValue(buildResponse());
		vi.mocked(triggerAdminIpGeoDbUpdate).mockResolvedValue({
			partial: false,
			unreachable_nodes: [],
			nodes: [
				{ node_id: "node-tokyo", status: "accepted", message: null },
				{
					node_id: "node-osaka",
					status: "skipped",
					message: "external override is active",
				},
			],
		});
		renderPage();

		await screen.findByText("Update policy");
		fireEvent.click(screen.getByRole("button", { name: "Manual update" }));

		await waitFor(() => {
			expect(triggerAdminIpGeoDbUpdate).toHaveBeenCalledWith("admintoken");
		});
	});

	it("polls while any node update is running", async () => {
		const idleResponse = buildResponse();
		const runningResponse = buildResponse({
			nodes: [
				{
					...idleResponse.nodes[0],
					running: true,
					last_started_at: "2026-03-09T08:00:00Z",
				},
				idleResponse.nodes[1],
			],
		});
		vi.mocked(fetchAdminIpGeoDb)
			.mockResolvedValueOnce(runningResponse)
			.mockResolvedValueOnce(idleResponse);
		renderPage();

		expect(await screen.findByText("Update policy")).toBeInTheDocument();
		await waitFor(() => {
			expect(fetchAdminIpGeoDb).toHaveBeenCalledTimes(1);
		});
		await new Promise((resolve) => setTimeout(resolve, 2200));
		await waitFor(() => {
			expect(fetchAdminIpGeoDb).toHaveBeenCalledTimes(2);
		});
	}, 10_000);

	it("shows partial reachability and node errors", async () => {
		vi.mocked(fetchAdminIpGeoDb).mockResolvedValue(
			buildResponse({
				partial: true,
				unreachable_nodes: ["node-remote"],
				nodes: [
					{
						...buildResponse().nodes[0],
						mode: "missing",
						last_error: "city DB download failed",
					},
					buildResponse().nodes[1],
				],
			}),
		);
		renderPage();

		expect(
			await screen.findByText("Node status is partial."),
		).toBeInTheDocument();
		expect(
			screen.getByText("Unreachable nodes: node-remote"),
		).toBeInTheDocument();
		expect(screen.getByText("city DB download failed")).toBeInTheDocument();
		expect(screen.getByText("Missing")).toBeInTheDocument();
	});

	it("shows token-required state when admin token is missing", async () => {
		mockReadAdminToken.mockReturnValue("");
		renderPage();

		expect(await screen.findByText("需要管理员 Token")).toBeInTheDocument();
		expect(fetchAdminIpGeoDb).not.toHaveBeenCalled();
	});
});
