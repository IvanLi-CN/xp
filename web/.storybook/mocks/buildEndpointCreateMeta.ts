import type { AdminEndpointCreateRequest } from "../../src/api/adminEndpoints";
import type { AdminNode } from "../../src/api/adminNodes";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

export function buildEndpointCreateMeta(
	payload: AdminEndpointCreateRequest,
	nodes: AdminNode[],
): Record<string, unknown> {
	if (payload.kind !== fixtureCatalog.endpoint.vlessKind()) {
		return {};
	}

	if (payload.reality) {
		return {
			reality: fixtureCatalog.endpoint.reality(),
		};
	}

	const node = nodes.find((item) => item.node_id === payload.node_id);
	if (!node) throw new Error("node not found");
	return {
		reality: fixtureCatalog.endpoint.reality(),
		managed_default: true,
		canary_upstream: fixtureCatalog.canaryUpstream.httpsListener(),
		accepted_authorities: fixtureCatalog.authority.edgeExamplePort443(),
	};
}
