import type { ServiceMonitorSummary } from "../../src/api/adminServiceMonitors";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import { clone, errorResponse, jsonResponse } from "./mockResponses";
import {
	type ResourceMonitoringMockState,
	handleResourceMonitoringMockRequest,
} from "./resourceMonitoringMock";
import {
	monitorDefinition,
	monitorHistory,
	monitorStatus,
} from "./serviceMonitorMock";

function findMonitor(
	serviceMonitors: ServiceMonitorSummary[],
	encodedMonitorId: string,
): ServiceMonitorSummary | undefined {
	return serviceMonitors.find(
		(monitor) => monitor.monitor_id === decodeURIComponent(encodedMonitorId),
	);
}

export function handleServiceMonitorMockRequest(
	serviceMonitors: ServiceMonitorSummary[],
	method: string,
	url: URL,
	resourceMonitoringState?: ResourceMonitoringMockState,
): Response | undefined {
	const path = url.pathname;
	if (path === "/api/admin/monitor-draft-tests" && method === "POST") {
		const monitor = serviceMonitors[0];
		return jsonResponse(
			{
				run_id: fixtureCatalog.identifier.probeRunPrimary(),
				target: monitor?.target,
				observer_policy: { mode: "exclude", node_ids: [] },
				observer_node_ids: [fixtureCatalog.identifier.nodePrimary()],
				coordinator_node_id: fixtureCatalog.identifier.nodePrimary(),
				state: "succeeded",
				created_at_unix_seconds: 1_700_000_000,
				expires_at_unix_seconds: 1_700_000_900,
				observers: [
					{
						node_id: fixtureCatalog.identifier.nodePrimary(),
						state: "succeeded",
						latency_ms: fixtureCatalog.number.value42(),
						status_code: 200,
					},
				],
			},
			{ status: 202 },
		);
	}
	if (path.startsWith("/api/admin/monitor-draft-tests/") && method === "GET") {
		const monitor = serviceMonitors[0];
		return jsonResponse({
			run_id: fixtureCatalog.identifier.probeRunPrimary(),
			target: monitor?.target,
			observer_policy: { mode: "exclude", node_ids: [] },
			observer_node_ids: [fixtureCatalog.identifier.nodePrimary()],
			coordinator_node_id: fixtureCatalog.identifier.nodePrimary(),
			state: "succeeded",
			created_at_unix_seconds: 1_700_000_000,
			expires_at_unix_seconds: 1_700_000_900,
			observers: [
				{
					node_id: fixtureCatalog.identifier.nodePrimary(),
					state: "succeeded",
					latency_ms: fixtureCatalog.number.value42(),
					status_code: 200,
				},
			],
		});
	}
	if (resourceMonitoringState) {
		const resourceResponse = handleResourceMonitoringMockRequest(
			resourceMonitoringState,
			method,
			path,
			url.searchParams,
		);
		if (resourceResponse) return resourceResponse;
	}
	if (path === "/api/admin/monitors" && method === "GET") {
		return jsonResponse({ items: clone(serviceMonitors) });
	}

	const statusMatch = path.match(/^\/api\/admin\/monitors\/([^/]+)\/status$/);
	if (statusMatch && method === "GET") {
		const monitor = findMonitor(serviceMonitors, statusMatch[1]);
		return monitor
			? jsonResponse(monitorStatus(monitor))
			: errorResponse(404, "not_found", "service monitor not found");
	}

	const historyMatch = path.match(/^\/api\/admin\/monitors\/([^/]+)\/history$/);
	if (historyMatch && method === "GET") {
		const monitor = findMonitor(serviceMonitors, historyMatch[1]);
		return monitor
			? jsonResponse(monitorHistory(monitor))
			: errorResponse(404, "not_found", "service monitor not found");
	}

	const monitorMatch = path.match(/^\/api\/admin\/monitors\/([^/]+)$/);
	if (monitorMatch && method === "GET") {
		const monitor = findMonitor(serviceMonitors, monitorMatch[1]);
		return monitor
			? jsonResponse(monitorDefinition(monitor))
			: errorResponse(404, "not_found", "service monitor not found");
	}

	return undefined;
}
