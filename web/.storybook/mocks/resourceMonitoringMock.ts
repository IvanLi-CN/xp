import type {
	ResourceHistoryResponse,
	ResourceRole,
	ResourceSnapshot,
} from "../../src/api/adminResources";
import { ResourceRoleSchema } from "../../src/api/adminResources";

import type {
	MockStateSeed,
	ResourceMonitoringMockError,
} from "./apiMockContract";
import { clone, errorResponse, jsonResponse } from "./mockResponses";

export type ResourceMonitoringMockState = Pick<
	MockStateSeed,
	| "nodeResourcesByNodeId"
	| "nodeResourceHistoryByNodeId"
	| "nodeResourceRuntimeHistoryByNodeId"
	| "resourceMonitoringErrors"
>;

function errorFixtureResponse(fixture: ResourceMonitoringMockError): Response {
	const response = errorResponse(
		fixture.status,
		fixture.code,
		fixture.message,
		fixture.details,
	);
	for (const [name, value] of Object.entries(fixture.headers ?? {})) {
		response.headers.set(name, value);
	}
	return response;
}

export function handleResourceMonitoringMockRequest(
	state: ResourceMonitoringMockState,
	method: string,
	path: string,
	searchParams: URLSearchParams,
): Response | null {
	if (method !== "GET") return null;

	const historyMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/resources\/history$/,
	);
	if (historyMatch) {
		const nodeId = decodeURIComponent(historyMatch[1]);
		const metric = searchParams.get("metric");
		const role = searchParams.get("role");
		const runtimeRole = ResourceRoleSchema.safeParse(role).data as
			| ResourceRole
			| undefined;
		const error = role
			? runtimeRole
				? state.resourceMonitoringErrors?.[nodeId]?.runtimeHistory?.[
						runtimeRole
					]?.[metric ?? ""]
				: undefined
			: state.resourceMonitoringErrors?.[nodeId]?.history?.[metric ?? ""];
		if (error) return errorFixtureResponse(error);
		const history = metric
			? role
				? runtimeRole
					? state.nodeResourceRuntimeHistoryByNodeId?.[nodeId]?.[runtimeRole]?.[
							metric
						]
					: undefined
				: state.nodeResourceHistoryByNodeId?.[nodeId]?.[metric]
			: undefined;
		return history
			? jsonResponse(clone(history))
			: errorResponse(
					404,
					"not_found",
					"resource history fixture is unavailable",
				);
	}

	const snapshotMatch = path.match(/^\/api\/admin\/nodes\/([^/]+)\/resources$/);
	if (!snapshotMatch) return null;
	const nodeId = decodeURIComponent(snapshotMatch[1]);
	const error = state.resourceMonitoringErrors?.[nodeId]?.snapshot;
	if (error) return errorFixtureResponse(error);
	const snapshot = state.nodeResourcesByNodeId?.[nodeId];
	return snapshot
		? jsonResponse(clone(snapshot))
		: errorResponse(
				404,
				"not_found",
				"resource snapshot fixture is unavailable",
			);
}

export type ResourceMonitoringMockFixtures = {
	snapshot: ResourceSnapshot;
	historyByMetric: Record<string, ResourceHistoryResponse>;
	runtimeHistoryByRole?: Record<
		string,
		Record<string, ResourceHistoryResponse>
	>;
};
