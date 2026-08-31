import type { ServiceMonitorSummary } from "../../src/api/adminServiceMonitors";
import { clone, errorResponse, jsonResponse } from "./mockResponses";
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
	path: string,
): Response | undefined {
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
