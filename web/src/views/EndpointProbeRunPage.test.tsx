import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { fetchAdminEndpointProbeRunStatus } from "../api/adminEndpointProbes";
import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { startSseStream } from "../api/sse";
import { ToastProvider } from "../components/Toast";
import { UiPrefsProvider } from "../components/UiPrefs";
import { createQueryClient } from "../queryClient";
import { EndpointProbeRunPage } from "./EndpointProbeRunPage";

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
		useNavigate: () => vi.fn(),
		useParams: () => ({
			runId: fixtureCatalog.identifier.probeRunPrimary(),
		}),
	};
});

vi.mock("../api/adminEndpointProbes", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("../api/adminEndpointProbes")>();
	return {
		...actual,
		fetchAdminEndpointProbeRunStatus: vi.fn(),
	};
});
vi.mock("../api/adminEndpoints");
vi.mock("../api/adminNodes");
vi.mock("../api/sse", () => ({
	startSseStream: vi.fn(),
}));
vi.mock("../api/sse", () => ({
	startSseStream: vi.fn(),
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
					<EndpointProbeRunPage />
				</ToastProvider>
			</UiPrefsProvider>
		</QueryClientProvider>,
	);
}

describe("<EndpointProbeRunPage />", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		vi.mocked(fetchAdminEndpointProbeRunStatus).mockResolvedValue({
			run_id: fixtureCatalog.identifier.probeRunPrimary(),
			status: "finished",
			hour: fixtureCatalog.timestamp.probeHour(),
			config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
			nodes: [
				{
					node_id: fixtureCatalog.nodeId.fixture106(),
					status: "finished",
					progress: {
						run_id: fixtureCatalog.identifier.probeRunPrimary(),
						hour: fixtureCatalog.timestamp.probeHour(),
						config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
						status: "finished",
						endpoints_total: 1,
						endpoints_done: 1,
						started_at: fixtureCatalog.timestamp.baseline(),
						updated_at: fixtureCatalog.timestamp.t20260311T110010(),
						finished_at: fixtureCatalog.timestamp.t20260311T110010(),
					},
				},
				{
					node_id: fixtureCatalog.nodeId.fixture110(),
					status: "finished",
					progress: {
						run_id: fixtureCatalog.identifier.probeRunPrimary(),
						hour: fixtureCatalog.timestamp.probeHour(),
						config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
						status: "finished",
						endpoints_total: 1,
						endpoints_done: 1,
						started_at: fixtureCatalog.timestamp.baseline(),
						updated_at: fixtureCatalog.timestamp.t20260311T110011(),
						finished_at: fixtureCatalog.timestamp.t20260311T110011(),
					},
				},
				{
					node_id: fixtureCatalog.nodeId.fixture224(),
					status: "busy",
					current: {
						run_id: fixtureCatalog.identifier.probeRunSecondary(),
						hour: fixtureCatalog.timestamp.probeHour(),
						config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
						status: "running",
						endpoints_total: 1,
						endpoints_done: 0,
						started_at: fixtureCatalog.timestamp.baseline(),
						updated_at: fixtureCatalog.timestamp.t20260311T110005(),
					},
				},
			],
		});
		vi.mocked(fetchAdminEndpoints).mockResolvedValue({
			items: [
				{
					endpoint_id: fixtureCatalog.endpointId.fixture40(),
					node_id: fixtureCatalog.nodeId.fixture106(),
					tag: fixtureCatalog.endpointId.fixture40(),
					kind: fixtureCatalog.endpoint.ssKind(),
					port: 443,
					meta: {},
					probe: {
						latest_checked_at: fixtureCatalog.timestamp.t20260311T110011(),
						latest_latency_ms_p50: 120,
						slots: [
							{
								hour: fixtureCatalog.timestamp.probeHour(),
								status: "missing",
								checked_at: fixtureCatalog.timestamp.t20260311T110011(),
								latency_ms_p50: 120,
							},
						],
					},
				},
			],
		});
		vi.mocked(fetchAdminNodes).mockResolvedValue({
			items: [
				{
					node_id: fixtureCatalog.nodeId.fixture106(),
					node_name: fixtureCatalog.nodeName.fixture210(),
					api_base_url: fixtureCatalog.service.fixture211(),
					access_host: fixtureCatalog.host.fixture212(),
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
				},
				{
					node_id: fixtureCatalog.nodeId.fixture110(),
					node_name: fixtureCatalog.nodeName.fixture207(),
					api_base_url: fixtureCatalog.service.fixture208(),
					access_host: fixtureCatalog.host.fixture209(),
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
				},
				{
					node_id: fixtureCatalog.nodeId.fixture224(),
					node_name: fixtureCatalog.nodeName.fixture226(),
					api_base_url: fixtureCatalog.service.fixture227(),
					access_host: fixtureCatalog.host.fixture228(),
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
				},
			],
		});
		vi.mocked(startSseStream).mockImplementation(({ onOpen, onMessage }) => {
			onOpen?.();
			onMessage?.({
				event: "sample",
				data: JSON.stringify({
					node_id: fixtureCatalog.nodeId.fixture106(),
					run_id: fixtureCatalog.identifier.probeRunPrimary(),
					hour: fixtureCatalog.timestamp.probeHour(),
					sample: {
						endpoint_id: fixtureCatalog.endpointId.fixture40(),
						ok: true,
						checked_at: fixtureCatalog.timestamp.t20260311T110010(),
						latency_ms: fixtureCatalog.number.value110(),
						config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
					},
				}),
			});
			onMessage?.({
				event: "sample",
				data: JSON.stringify({
					node_id: fixtureCatalog.nodeId.fixture110(),
					run_id: fixtureCatalog.identifier.probeRunPrimary(),
					hour: fixtureCatalog.timestamp.probeHour(),
					sample: {
						endpoint_id: fixtureCatalog.endpointId.fixture40(),
						ok: true,
						checked_at: fixtureCatalog.timestamp.t20260311T110011(),
						latency_ms: fixtureCatalog.number.value120(),
						config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
					},
				}),
			});
			return { close: vi.fn() };
		});
	});

	it("computes live endpoint status from participating run nodes only", async () => {
		renderPage();

		await waitFor(() => {
			expect(fetchAdminEndpointProbeRunStatus).toHaveBeenCalledWith(
				"admintoken",
				fixtureCatalog.identifier.probeRunPrimary(),
				expect.any(AbortSignal),
			);
		});

		expect(
			(await screen.findAllByText(fixtureCatalog.endpointId.fixture40()))
				.length,
		).toBeGreaterThan(0);
		expect(await screen.findByText("Up")).toBeInTheDocument();
		expect(screen.queryByText("Missing")).toBeNull();
	});

	it("shows node runners by linked display name", async () => {
		renderPage();

		const links = await screen.findAllByRole("link", {
			name: /Open node details:/i,
		});
		expect(links.map((link) => link.textContent)).toEqual([
			fixtureCatalog.nodeName.fixture207(),
			fixtureCatalog.nodeName.fixture210(),
			fixtureCatalog.nodeName.fixture226(),
		]);
		expect(links.map((link) => link.getAttribute("href"))).toEqual([
			`/nodes/${fixtureCatalog.nodeId.fixture110()}`,
			`/nodes/${fixtureCatalog.nodeId.fixture106()}`,
			`/nodes/${fixtureCatalog.nodeId.fixture224()}`,
		]);
	});

	it("falls back to runner IDs when the node-name lookup fails", async () => {
		vi.mocked(fetchAdminNodes).mockRejectedValueOnce(
			new Error("nodes unavailable"),
		);
		renderPage();

		expect(
			await screen.findByText(fixtureCatalog.nodeId.fixture106()),
		).toBeInTheDocument();
		expect(screen.queryByText("Failed to load probe status")).toBeNull();
	});
});
