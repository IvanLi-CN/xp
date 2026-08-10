import type { QueryClient } from "@tanstack/react-query";

import type {
	AdminEndpoint,
	AdminEndpointsResponse,
} from "../api/adminEndpoints";
import type {
	AdminNodePatchRequest,
	AdminNodesResponse,
} from "../api/adminNodes";

export function appendAdminEndpoint(
	previous: AdminEndpointsResponse | undefined,
	endpoint: AdminEndpoint,
) {
	return previous
		? { ...previous, items: [...previous.items, endpoint] }
		: previous;
}

export function replaceAdminEndpoint(
	previous: AdminEndpointsResponse | undefined,
	endpoint: AdminEndpoint,
) {
	return previous
		? {
				...previous,
				items: previous.items.map((item) =>
					item.endpoint_id === endpoint.endpoint_id ? endpoint : item,
				),
			}
		: previous;
}

export function removeAdminEndpoints(
	previous: AdminEndpointsResponse | undefined,
	endpointIds: readonly string[],
) {
	if (!previous || endpointIds.length === 0) return previous;
	const ids = new Set(endpointIds);
	return {
		...previous,
		items: previous.items.filter((item) => !ids.has(item.endpoint_id)),
	};
}

function removeAdminEndpointsForNode(
	previous: AdminEndpointsResponse | undefined,
	nodeId: string,
) {
	return previous
		? {
				...previous,
				items: previous.items.filter((item) => item.node_id !== nodeId),
			}
		: previous;
}

export function syncNode(
	queryClient: QueryClient,
	adminToken: string,
	nodeId: string,
	patch: AdminNodePatchRequest,
) {
	queryClient.setQueryData<AdminNodesResponse>(
		["adminNodes", adminToken],
		(previous) =>
			previous
				? {
						...previous,
						items: previous.items.map((item) =>
							item.node_id === nodeId ? { ...item, ...patch } : item,
						),
					}
				: previous,
	);
	void queryClient.invalidateQueries({
		queryKey: ["adminNodes", adminToken],
	});
}

export const resourceListCache = {
	append: appendAdminEndpoint,
	update(
		queryClient: QueryClient,
		adminToken: string,
		endpoint: AdminEndpoint,
	) {
		queryClient.setQueryData<AdminEndpointsResponse>(
			["adminEndpoints", adminToken],
			(previous) => replaceAdminEndpoint(previous, endpoint),
		);
		void queryClient.invalidateQueries({
			queryKey: ["adminEndpoints", adminToken],
		});
	},
	remove(queryClient: QueryClient, adminToken: string, endpointId: string) {
		queryClient.setQueryData<AdminEndpointsResponse>(
			["adminEndpoints", adminToken],
			(previous) => removeAdminEndpoints(previous, [endpointId]),
		);
		void queryClient.invalidateQueries({
			queryKey: ["adminEndpoints", adminToken],
		});
		queryClient.removeQueries({
			queryKey: ["adminEndpoint", adminToken, endpointId],
		});
	},
	nodeDeleted(
		queryClient: QueryClient,
		adminToken: string,
		nodeId: string,
		endpoints: ReadonlyArray<{ endpoint_id: string }>,
	) {
		queryClient.setQueryData<AdminNodesResponse>(
			["adminNodes", adminToken],
			(previous) =>
				previous
					? {
							...previous,
							items: previous.items.filter((item) => item.node_id !== nodeId),
						}
					: previous,
		);
		queryClient.setQueryData<AdminEndpointsResponse>(
			["adminEndpoints", adminToken],
			(previous) => removeAdminEndpointsForNode(previous, nodeId),
		);
		void queryClient.invalidateQueries({
			queryKey: ["adminNodes", adminToken],
		});
		void queryClient.invalidateQueries({
			queryKey: ["adminEndpoints", adminToken],
		});
		queryClient.removeQueries({
			queryKey: ["adminNode", adminToken, nodeId],
		});
		for (const endpoint of endpoints) {
			queryClient.removeQueries({
				queryKey: ["adminEndpoint", adminToken, endpoint.endpoint_id],
			});
		}
	},
};
