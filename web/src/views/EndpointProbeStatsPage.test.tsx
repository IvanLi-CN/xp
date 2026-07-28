import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminEndpointProbeHistory } from "../api/adminEndpointProbes";
import { fetchAdminEndpoint } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { EndpointProbeStatsPage } from "./EndpointProbeStatsPage";

vi.mock("@tanstack/react-router", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("@tanstack/react-router")>();
	return {
		...actual,
		Link: ({
			children,
			to,
			params,
			...rest
		}: {
			children: React.ReactNode;
			to?: string;
			params?: Record<string, string>;
		}) => {
			let href = to ?? "#";
			if (params) {
				for (const [key, value] of Object.entries(params)) {
					href = href.replace(`$${key}`, value);
				}
			}
			return (
				<a href={href} {...rest}>
					{children}
				</a>
			);
		},
		useParams: () => ({ endpointId: "endpoint-1" }),
	};
});

vi.mock("../api/adminEndpointProbes");
vi.mock("../api/adminEndpoints");
vi.mock("../api/adminNodes");

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
				<EndpointProbeStatsPage />
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

describe("<EndpointProbeStatsPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		vi.mocked(fetchAdminEndpoint).mockResolvedValue({
			endpoint_id: "endpoint-1",
			node_id: "node-1",
			tag: "tokyo-vless",
			kind: "ss2022_2022_blake3_aes_128_gcm",
			port: 443,
			meta: {},
		});
		vi.mocked(fetchAdminEndpointProbeHistory).mockResolvedValue({
			endpoint_id: "endpoint-1",
			participating_nodes: 2,
			expected_nodes: 2,
			slots: [
				{
					hour: "2026-03-11T11:00:00Z",
					status: "up",
					participating_nodes: 2,
					ok_count: 2,
					sample_count: 2,
					skipped_count: 0,
					tested_count: 2,
					latency_ms_p50: 111,
					latency_ms_p95: 123,
					by_node: [
						{
							node_id: "node-1",
							ok: true,
							skipped: false,
							checked_at: "2026-03-11T11:05:00Z",
							latency_ms: 111,
							config_hash: "cfg",
						},
						{
							node_id: "node-2",
							ok: true,
							skipped: false,
							checked_at: "2026-03-11T11:05:01Z",
							latency_ms: 123,
							config_hash: "cfg",
						},
					],
				},
			],
		});
		vi.mocked(fetchAdminNodes).mockResolvedValue({
			items: [
				{
					node_id: "node-1",
					node_name: "Tokyo edge",
					api_base_url: "https://tokyo.example.invalid",
					access_host: "tokyo.example.invalid",
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
				},
				{
					node_id: "node-2",
					node_name: "Amsterdam edge",
					api_base_url: "https://amsterdam.example.invalid",
					access_host: "amsterdam.example.invalid",
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
				},
			],
		});
	});

	it("shows participant-based labels and reported counts", async () => {
		renderPage();

		await waitFor(() => {
			expect(fetchAdminEndpointProbeHistory).toHaveBeenCalledWith(
				"admintoken",
				"endpoint-1",
				24,
				expect.any(AbortSignal),
			);
		});

		expect(
			await screen.findByText(/Participating nodes:/i),
		).toBeInTheDocument();
		expect(screen.getAllByText("2/2").length).toBeGreaterThan(0);
		expect(screen.queryByText(/Expected nodes:/i)).toBeNull();
	});

	it("falls back to expected_nodes for legacy probe-history payloads", async () => {
		vi.mocked(fetchAdminEndpointProbeHistory).mockResolvedValueOnce({
			endpoint_id: "endpoint-1",
			expected_nodes: 2,
			slots: [
				{
					hour: "2026-03-11T11:00:00Z",
					status: "up",
					ok_count: 2,
					sample_count: 2,
					skipped_count: 0,
					tested_count: 2,
					latency_ms_p50: 111,
					latency_ms_p95: 123,
					by_node: [
						{
							node_id: "node-1",
							ok: true,
							skipped: false,
							checked_at: "2026-03-11T11:05:00Z",
							latency_ms: 111,
							config_hash: "cfg",
						},
						{
							node_id: "node-2",
							ok: true,
							skipped: false,
							checked_at: "2026-03-11T11:05:01Z",
							latency_ms: 123,
							config_hash: "cfg",
						},
					],
				},
			],
		});

		renderPage();

		expect(
			await screen.findByText(/Participating nodes:/i),
		).toBeInTheDocument();
		expect(screen.getAllByText("2/2").length).toBeGreaterThan(0);
	});

	it("shows linked node names in display-name order", async () => {
		renderPage();

		const links = await screen.findAllByRole("link", {
			name: /Open node details:/i,
		});
		expect(links.map((link) => link.textContent)).toEqual([
			"Amsterdam edge",
			"Tokyo edge",
		]);
		expect(links.map((link) => link.getAttribute("href"))).toEqual([
			"/nodes/node-2",
			"/nodes/node-1",
		]);
		expect(links[0]).toHaveAttribute("title", "node-2");
	});

	it("keeps probe stats available when the node-name lookup fails", async () => {
		vi.mocked(fetchAdminNodes).mockRejectedValueOnce(
			new Error("nodes unavailable"),
		);
		renderPage();

		expect(await screen.findByText("node-1")).toBeInTheDocument();
		expect(screen.queryByText("Failed to load probe stats")).toBeNull();
	});
});
