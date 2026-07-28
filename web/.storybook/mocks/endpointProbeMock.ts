import type {
	AdminEndpointProbeHistoryResponse,
	AdminEndpointProbeRunStatusResponse,
} from "../../src/api/adminEndpointProbes";

export type StorybookEndpointProbeMock = {
	historyByEndpointId?: Record<string, AdminEndpointProbeHistoryResponse>;
	runsByRunId?: Record<string, AdminEndpointProbeRunStatusResponse>;
};

function jsonResponse(data: unknown): Response {
	return new Response(JSON.stringify(data), {
		status: 200,
		headers: { "Content-Type": "application/json" },
	});
}

function notFound(message: string): Response {
	return jsonResponse({ error: { code: "not_found", message, details: {} } });
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
		const history = probe?.historyByEndpointId?.[endpointId];
		return history
			? jsonResponse(history)
			: notFound("probe history not found");
	}

	const eventsMatch = path.match(
		/^\/api\/admin\/endpoints\/probe\/runs\/([^/]+)\/events$/,
	);
	if (eventsMatch && method === "GET") {
		const runId = decodeURIComponent(eventsMatch[1]);
		const run = probe?.runsByRunId?.[runId];
		if (!run) return notFound("probe run not found");
		return sseResponse([
			{
				event: "hello",
				data: {
					run_id: run.run_id,
					connected_at: "2026-07-29T08:00:00Z",
					nodes: run.nodes.map((node) => node.node_id),
				},
			},
		]);
	}

	const runMatch = path.match(
		/^\/api\/admin\/endpoints\/probe\/runs\/([^/]+)$/,
	);
	if (runMatch && method === "GET") {
		const runId = decodeURIComponent(runMatch[1]);
		const run = probe?.runsByRunId?.[runId];
		return run ? jsonResponse(run) : notFound("probe run not found");
	}

	return undefined;
}
