import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAdminEndpointProbeRunStatus } from "../api/adminEndpointProbes";
import { fetchAdminEndpoints } from "../api/adminEndpoints";
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
					node_id: "node-a",
					status: "finished",
					progress: {
						run_id: "run-1",
						hour: "2026-03-11T11:00:00Z",
						config_hash: "cfg-hash",
						status: "finished",
						endpoints_total: 1,
						endpoints_done: 1,
						started_at: "2026-03-11T11:00:00Z",
						updated_at: "2026-03-11T11:00:10Z",
						finished_at: "2026-03-11T11:00:10Z",
					},
				},
				{
					node_id: "node-b",
					status: "finished",
					progress: {
						run_id: "run-1",
						hour: "2026-03-11T11:00:00Z",
						config_hash: "cfg-hash",
						status: "finished",
						endpoints_total: 1,
						endpoints_done: 1,
						started_at: "2026-03-11T11:00:00Z",
						updated_at: "2026-03-11T11:00:11Z",
						finished_at: "2026-03-11T11:00:11Z",
					},
				},
				{
					node_id: "node-c",
					status: "busy",
					current: {
						run_id: "run-elsewhere",
						hour: "2026-03-11T11:00:00Z",
						config_hash: "cfg-hash",
						status: "running",
						endpoints_total: 1,
						endpoints_done: 0,
						started_at: "2026-03-11T11:00:00Z",
						updated_at: "2026-03-11T11:00:05Z",
					},
				},
			],
		});
		vi.mocked(fetchAdminEndpoints).mockResolvedValue({
			items: [
				{
					endpoint_id: "endpoint-1",
					node_id: "node-a",
					tag: "endpoint-1",
					kind: "ss2022_2022_blake3_aes_128_gcm",
					port: 443,
					meta: {},
					probe: {
						latest_checked_at: "2026-03-11T11:00:11Z",
						latest_latency_ms_p50: 120,
						slots: [
							{
								hour: "2026-03-11T11:00:00Z",
								status: "missing",
								checked_at: "2026-03-11T11:00:11Z",
								latency_ms_p50: 120,
							},
						],
					},
				},
			],
		});
		vi.mocked(startSseStream).mockImplementation(({ onOpen, onMessage }) => {
			onOpen?.();
			onMessage?.({
				event: "sample",
				data: JSON.stringify({
					node_id: "node-a",
					run_id: "run-1",
					hour: "2026-03-11T11:00:00Z",
					sample: {
						endpoint_id: "endpoint-1",
						ok: true,
						checked_at: "2026-03-11T11:00:10Z",
						latency_ms: 110,
						config_hash: "cfg-hash",
					},
				}),
			});
			onMessage?.({
				event: "sample",
				data: JSON.stringify({
					node_id: "node-b",
					run_id: "run-1",
					hour: "2026-03-11T11:00:00Z",
					sample: {
						endpoint_id: "endpoint-1",
						ok: true,
						checked_at: "2026-03-11T11:00:11Z",
						latency_ms: 120,
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

		expect((await screen.findAllByText("endpoint-1")).length).toBeGreaterThan(
			0,
		);
		expect(await screen.findByText("Up")).toBeInTheDocument();
		expect(screen.queryByText("Missing")).toBeNull();
	});
});
