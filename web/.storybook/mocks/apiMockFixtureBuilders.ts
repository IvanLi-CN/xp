import type { AdminEndpoint } from "../../src/api/adminEndpoints";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminUserAccessItem } from "../../src/api/adminUserAccess";
import type { AdminUserTokenResponse } from "../../src/api/adminUsers";
import type { ClusterInfoResponse } from "../../src/api/clusterInfo";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import { buildNodeDeletePreviewEndpoint } from "./staticFixtureMappings";

export function catalogTimestampOffset(offsetMilliseconds: number): string {
	return new Date(
		Date.parse(fixtureCatalog.timestamp.baseline()) + offsetMilliseconds,
	).toISOString();
}

export function buildNodeDeletePreviewResponse(
	node: AdminNode,
	endpoints: AdminEndpoint[],
) {
	return {
		node_id: node.node_id,
		endpoints: endpoints
			.filter((endpoint) => endpoint.node_id === node.node_id)
			.map(buildNodeDeletePreviewEndpoint),
	};
}

export function buildNodeEgressProbeRefreshResponse(node: AdminNode) {
	return {
		node_id: node.node_id,
		accepted: true,
		egress_probe: node.egress_probe,
	};
}

export function buildStatusHelloResponse(clusterInfo: ClusterInfoResponse) {
	return {
		node_id: clusterInfo.node_id,
		connected_at: fixtureCatalog.slotString.s7(),
	};
}

export function buildUserAccessItem(
	userId: string,
	endpoint: Pick<AdminEndpoint, "endpoint_id" | "node_id">,
): AdminUserAccessItem {
	return {
		user_id: userId,
		endpoint_id: endpoint.endpoint_id,
		node_id: endpoint.node_id,
	};
}

export function buildSubscriptionTokenResponse(
	token: string,
): AdminUserTokenResponse {
	return { subscription_token: token };
}
