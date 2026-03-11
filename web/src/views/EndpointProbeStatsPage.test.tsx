import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminEndpointProbeHistory } from "../api/adminEndpointProbes";
import { fetchAdminEndpoint } from "../api/adminEndpoints";
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
});
