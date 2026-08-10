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
		useParams: () => ({ endpointId: fixtureCatalog.endpointId.fixture40() }),
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
			endpoint_id: fixtureCatalog.endpointId.fixture40(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			tag: fixtureCatalog.endpointTag.fixture139(),
			kind: fixtureCatalog.endpoint.ssKind(),
			port: fixtureCatalog.endpoint.port443(),
			meta: {},
		});
		vi.mocked(fetchAdminEndpointProbeHistory).mockResolvedValue({
			endpoint_id: fixtureCatalog.endpointId.fixture40(),
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
							node_id: fixtureCatalog.nodeId.fixture32(),
							ok: true,
							skipped: false,
							checked_at: fixtureCatalog.timestamp.t20260311T110500(),
							latency_ms: fixtureCatalog.number.value111(),
							config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
						},
						{
							node_id: fixtureCatalog.nodeId.fixture36(),
							ok: true,
							skipped: false,
							checked_at: fixtureCatalog.timestamp.t20260311T110501(),
							latency_ms: fixtureCatalog.number.value123(),
							config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
						},
					],
				},
			],
		});
		vi.mocked(fetchAdminNodes).mockResolvedValue({
			items: [
				{
					node_id: fixtureCatalog.nodeId.fixture32(),
					node_name: fixtureCatalog.nodeName.fixture210(),
					api_base_url: fixtureCatalog.service.fixture211(),
					access_host: fixtureCatalog.host.fixture212(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.resetUnlimited(),
				},
				{
					node_id: fixtureCatalog.nodeId.fixture36(),
					node_name: fixtureCatalog.nodeName.fixture207(),
					api_base_url: fixtureCatalog.service.fixture208(),
					access_host: fixtureCatalog.host.fixture209(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.resetUnlimited(),
				},
			],
		});
	});

	it("shows participant-based labels and reported counts", async () => {
		renderPage();

		await waitFor(() => {
			expect(fetchAdminEndpointProbeHistory).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.endpointId.fixture40(),
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
			endpoint_id: fixtureCatalog.endpointId.fixture40(),
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
							node_id: fixtureCatalog.nodeId.fixture32(),
							ok: true,
							skipped: false,
							checked_at: fixtureCatalog.timestamp.t20260311T110500(),
							latency_ms: fixtureCatalog.number.value111(),
							config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
						},
						{
							node_id: fixtureCatalog.nodeId.fixture36(),
							ok: true,
							skipped: false,
							checked_at: fixtureCatalog.timestamp.t20260311T110501(),
							latency_ms: fixtureCatalog.number.value123(),
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
			fixtureCatalog.nodeName.fixture207(),
			fixtureCatalog.nodeName.fixture210(),
		]);
		expect(links.map((link) => link.getAttribute("href"))).toEqual([
			`/nodes/${fixtureCatalog.nodeId.fixture36()}`,
			`/nodes/${fixtureCatalog.nodeId.fixture32()}`,
		]);
		expect(links[0]).toHaveAttribute(
			"title",
			fixtureCatalog.nodeId.fixture36(),
		);
	});

	it("keeps probe stats available when the node-name lookup fails", async () => {
		vi.mocked(fetchAdminNodes).mockRejectedValueOnce(
			new Error("nodes unavailable"),
		);
		renderPage();

		expect(
			await screen.findByText(fixtureCatalog.nodeId.fixture32()),
		).toBeInTheDocument();
		expect(screen.queryByText("Failed to load probe stats")).toBeNull();
	});
});
