import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

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
		useParams: () => ({ endpointId: fixtureCatalog.slotString.s40() }),
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
			endpoint_id: fixtureCatalog.slotString.s40(),
			node_id: fixtureCatalog.slotString.s32(),
			tag: fixtureCatalog.slotString.s139(),
			kind: "ss2022_2022_blake3_aes_128_gcm",
			port: 443,
			meta: {},
		});
		vi.mocked(fetchAdminEndpointProbeHistory).mockResolvedValue({
			endpoint_id: fixtureCatalog.slotString.s40(),
			participating_nodes: 2,
			expected_nodes: 2,
			slots: [
				{
					hour: fixtureCatalog.timestamp.probeHour(),
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
							node_id: fixtureCatalog.slotString.s32(),
							ok: true,
							skipped: false,
							checked_at: fixtureCatalog.slotString.s236(),
							latency_ms: fixtureCatalog.slotNumber.n14(),
							config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
						},
						{
							node_id: fixtureCatalog.slotString.s36(),
							ok: true,
							skipped: false,
							checked_at: fixtureCatalog.slotString.s237(),
							latency_ms: fixtureCatalog.slotNumber.n15(),
							config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
						},
					],
				},
			],
		});
		vi.mocked(fetchAdminNodes).mockResolvedValue({
			items: [
				{
					node_id: fixtureCatalog.slotString.s32(),
					node_name: fixtureCatalog.slotString.s210(),
					api_base_url: fixtureCatalog.slotString.s211(),
					access_host: fixtureCatalog.slotString.s212(),
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
				},
				{
					node_id: fixtureCatalog.slotString.s36(),
					node_name: fixtureCatalog.slotString.s207(),
					api_base_url: fixtureCatalog.slotString.s208(),
					access_host: fixtureCatalog.slotString.s209(),
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
				fixtureCatalog.slotString.s40(),
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
			endpoint_id: fixtureCatalog.slotString.s40(),
			expected_nodes: 2,
			slots: [
				{
					hour: fixtureCatalog.timestamp.probeHour(),
					status: "up",
					ok_count: 2,
					sample_count: 2,
					skipped_count: 0,
					tested_count: 2,
					latency_ms_p50: 111,
					latency_ms_p95: 123,
					by_node: [
						{
							node_id: fixtureCatalog.slotString.s32(),
							ok: true,
							skipped: false,
							checked_at: fixtureCatalog.slotString.s236(),
							latency_ms: fixtureCatalog.slotNumber.n14(),
							config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
						},
						{
							node_id: fixtureCatalog.slotString.s36(),
							ok: true,
							skipped: false,
							checked_at: fixtureCatalog.slotString.s237(),
							latency_ms: fixtureCatalog.slotNumber.n15(),
							config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
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
			fixtureCatalog.slotString.s207(),
			fixtureCatalog.slotString.s210(),
		]);
		expect(links.map((link) => link.getAttribute("href"))).toEqual([
			`/nodes/${fixtureCatalog.slotString.s36()}`,
			`/nodes/${fixtureCatalog.slotString.s32()}`,
		]);
		expect(links[0]).toHaveAttribute("title", fixtureCatalog.slotString.s36());
	});

	it("keeps probe stats available when the node-name lookup fails", async () => {
		vi.mocked(fetchAdminNodes).mockRejectedValueOnce(
			new Error("nodes unavailable"),
		);
		renderPage();

		expect(
			await screen.findByText(fixtureCatalog.slotString.s32()),
		).toBeInTheDocument();
		expect(screen.queryByText("Failed to load probe stats")).toBeNull();
	});
});
