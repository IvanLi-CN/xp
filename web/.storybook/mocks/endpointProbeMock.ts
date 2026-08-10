import type {
	AdminEndpointProbeHistoryResponse,
	AdminEndpointProbeRunStatusResponse,
} from "../../src/api/adminEndpointProbes";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

export type StorybookEndpointProbeMock = {
	historyByEndpointId?: Record<string, AdminEndpointProbeHistoryResponse>;
	runsByRunId?: Record<string, AdminEndpointProbeRunStatusResponse>;
};

function fixtureProbeHistory(): AdminEndpointProbeHistoryResponse {
	return {
		endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
		participating_nodes: fixtureCatalog.slotNumber.n1(),
		expected_nodes: fixtureCatalog.slotNumber.n1(),
		slots: [
			{
				hour: fixtureCatalog.timestamp.probeHour(),
				status: "up",
				participating_nodes: fixtureCatalog.slotNumber.n1(),
				ok_count: fixtureCatalog.slotNumber.n1(),
				sample_count: fixtureCatalog.slotNumber.n1(),
				latency_ms_p50: fixtureCatalog.metric.latencyLow(),
				latency_ms_p95: fixtureCatalog.metric.latencyHigh(),
				by_node: [
					{
						node_id: fixtureCatalog.identifier.nodePrimary(),
						ok: true,
						checked_at: fixtureCatalog.timestamp.probeLatest(),
						latency_ms: fixtureCatalog.metric.latencyLow(),
						config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
					},
				],
			},
		],
	};
}

function fixtureProbeRun(): AdminEndpointProbeRunStatusResponse {
	return {
		run_id: fixtureCatalog.identifier.probeRunPrimary(),
		status: "finished",
		hour: fixtureCatalog.timestamp.probeHour(),
		config_hash: fixtureCatalog.identifier.probeConfigPrimary(),
		nodes: [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				status: "finished",
			},
		],
	};
}

function jsonResponse(data: unknown): Response {
	return new Response(JSON.stringify(data), {
		status: 200,
		headers: { "Content-Type": "application/json" },
	});
}

function sseResponse(
	events: Array<{ event: string; data: unknown }>,
): Response {
	const body = events
		.map(
			(item) => `event: ${item.event}\ndata: ${JSON.stringify(item.data)}\n\n`,
		)
		.join("");
	return new Response(body, {
		status: 200,
		headers: {
			"Content-Type": "text/event-stream",
			"Cache-Control": "no-cache",
		},
	});
}

export function handleEndpointProbeRequest(
	req: Request,
	probe?: StorybookEndpointProbeMock,
): Response | undefined {
	const method = req.method.toUpperCase();
	const path = new URL(req.url, "http://localhost").pathname;
	const historyMatch = path.match(
		/^\/api\/admin\/endpoints\/([^/]+)\/probe-history$/,
	);
	if (historyMatch && method === "GET") {
		const endpointId = decodeURIComponent(historyMatch[1]);
		return jsonResponse(
			probe?.historyByEndpointId?.[endpointId] ?? fixtureProbeHistory(),
		);
	}

	const eventsMatch = path.match(
		/^\/api\/admin\/endpoints\/probe\/runs\/([^/]+)\/events$/,
	);
	if (eventsMatch && method === "GET") {
		return sseResponse([
			{
				event: "hello",
				data: {
					run_id: fixtureCatalog.identifier.probeRunPrimary(),
					connected_at: fixtureCatalog.slotString.s0(),
					nodes: [fixtureCatalog.identifier.nodePrimary()],
				},
			},
		]);
	}

	const runMatch = path.match(
		/^\/api\/admin\/endpoints\/probe\/runs\/([^/]+)$/,
	);
	if (runMatch && method === "GET") {
		const runId = decodeURIComponent(runMatch[1]);
		return jsonResponse(probe?.runsByRunId?.[runId] ?? fixtureProbeRun());
	}

	return undefined;
}
