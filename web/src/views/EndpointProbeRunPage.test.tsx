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
		useParams: () => ({ runId: "run-1" }),
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
			run_id: "run-1",
			status: "finished",
			hour: "2026-03-11T11:00:00Z",
			config_hash: "cfg-hash",
			nodes: [
				{
					node_id: fixtureCatalog.slotString.s106(),
					status: "finished",
					progress: {
						run_id: "run-1",
						hour: "2026-03-11T11:00:00Z",
						config_hash: "cfg-hash",
						status: "finished",
						endpoints_total: 1,
						endpoints_done: 1,
						started_at: "2026-03-11T11:00:00Z",
						updated_at: fixtureCatalog.slotString.s222(),
						finished_at: fixtureCatalog.slotString.s222(),
					},
				},
				{
					node_id: fixtureCatalog.slotString.s110(),
					status: "finished",
					progress: {
						run_id: "run-1",
						hour: "2026-03-11T11:00:00Z",
						config_hash: "cfg-hash",
						status: "finished",
						endpoints_total: 1,
						endpoints_done: 1,
						started_at: "2026-03-11T11:00:00Z",
						updated_at: fixtureCatalog.slotString.s223(),
						finished_at: fixtureCatalog.slotString.s223(),
					},
				},
				{
					node_id: fixtureCatalog.slotString.s224(),
					status: "busy",
					current: {
						run_id: "run-elsewhere",
						hour: "2026-03-11T11:00:00Z",
						config_hash: "cfg-hash",
						status: "running",
						endpoints_total: 1,
						endpoints_done: 0,
						started_at: "2026-03-11T11:00:00Z",
						updated_at: fixtureCatalog.slotString.s225(),
					},
				},
			],
		});
		vi.mocked(fetchAdminEndpoints).mockResolvedValue({
			items: [
				{
					endpoint_id: fixtureCatalog.slotString.s40(),
					node_id: fixtureCatalog.slotString.s106(),
					tag: fixtureCatalog.slotString.s40(),
					kind: "ss2022_2022_blake3_aes_128_gcm",
					port: 443,
					meta: {},
					probe: {
						latest_checked_at: fixtureCatalog.slotString.s223(),
						latest_latency_ms_p50: 120,
						slots: [
							{
								hour: "2026-03-11T11:00:00Z",
								status: "missing",
								checked_at: fixtureCatalog.slotString.s223(),
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
					node_id: fixtureCatalog.slotString.s106(),
					node_name: fixtureCatalog.slotString.s210(),
					api_base_url: fixtureCatalog.slotString.s211(),
					access_host: fixtureCatalog.slotString.s212(),
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
				},
				{
					node_id: fixtureCatalog.slotString.s110(),
					node_name: fixtureCatalog.slotString.s207(),
					api_base_url: fixtureCatalog.slotString.s208(),
					access_host: fixtureCatalog.slotString.s209(),
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
				},
				{
					node_id: fixtureCatalog.slotString.s224(),
					node_name: fixtureCatalog.slotString.s226(),
					api_base_url: fixtureCatalog.slotString.s227(),
					access_host: fixtureCatalog.slotString.s228(),
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
					node_id: fixtureCatalog.slotString.s106(),
					run_id: "run-1",
					hour: "2026-03-11T11:00:00Z",
					sample: {
						endpoint_id: fixtureCatalog.slotString.s40(),
						ok: true,
						checked_at: fixtureCatalog.slotString.s222(),
						latency_ms: fixtureCatalog.slotNumber.n12(),
						config_hash: "cfg-hash",
					},
				}),
			});
			onMessage?.({
				event: "sample",
				data: JSON.stringify({
					node_id: fixtureCatalog.slotString.s110(),
					run_id: "run-1",
					hour: "2026-03-11T11:00:00Z",
					sample: {
						endpoint_id: fixtureCatalog.slotString.s40(),
						ok: true,
						checked_at: fixtureCatalog.slotString.s223(),
						latency_ms: fixtureCatalog.slotNumber.n13(),
						config_hash: "cfg-hash",
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
				"run-1",
				expect.any(AbortSignal),
			);
		});

		expect(
			(await screen.findAllByText(fixtureCatalog.slotString.s40())).length,
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
			fixtureCatalog.slotString.s207(),
			fixtureCatalog.slotString.s210(),
			fixtureCatalog.slotString.s226(),
		]);
		expect(links.map((link) => link.getAttribute("href"))).toEqual([
			`/nodes/${fixtureCatalog.slotString.s110()}`,
			`/nodes/${fixtureCatalog.slotString.s106()}`,
			`/nodes/${fixtureCatalog.slotString.s224()}`,
		]);
	});

	it("falls back to runner IDs when the node-name lookup fails", async () => {
		vi.mocked(fetchAdminNodes).mockRejectedValueOnce(
			new Error("nodes unavailable"),
		);
		renderPage();

		expect(
			await screen.findByText(fixtureCatalog.slotString.s106()),
		).toBeInTheDocument();
		expect(screen.queryByText("Failed to load probe status")).toBeNull();
	});
});
