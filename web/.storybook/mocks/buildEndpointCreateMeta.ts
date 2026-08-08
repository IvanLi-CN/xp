import type { AdminEndpointCreateRequest } from "../../src/api/adminEndpoints";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminRealityDomain } from "../../src/api/adminRealityDomains";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

export function deriveGlobalServerNames(
	domains: AdminRealityDomain[],
	nodeId: string,
): string[] {
	const out: string[] = [];
	const seen = new Set<string>();
	for (const domain of domains) {
		if (domain.disabled_node_ids.includes(nodeId)) continue;
		const trimmed = domain.server_name.trim();
		if (!trimmed) continue;
		const key = trimmed.toLowerCase();
		if (seen.has(key)) continue;
		seen.add(key);
		out.push(trimmed);
	}
	return out;
}

export function buildEndpointCreateMeta(
	payload: AdminEndpointCreateRequest,
	nodes: AdminNode[],
	realityDomains: AdminRealityDomain[],
): Record<string, unknown> {
	if (payload.kind !== "vless_reality_vision_tcp") {
		return {};
	}

	if (payload.reality) {
		const source = payload.reality.server_names_source ?? "manual";
		const derived =
			source === "global"
				? deriveGlobalServerNames(realityDomains, payload.node_id)
				: payload.reality.server_names;
		if (!derived || derived.length === 0) {
			throw new Error("server_names must be non-empty");
		}
		return {
			reality: {
				...payload.reality,
				dest: fixtureCatalog.slotString.s1(),
				server_names: fixtureCatalog.slotList.l0(),
				server_names_source: source,
			},
		};
	}

	const node = nodes.find((item) => item.node_id === payload.node_id);
	if (!node) throw new Error("node not found");

	return {
		reality: {
			dest: fixtureCatalog.slotString.s2(),
			server_names: fixtureCatalog.slotList.l1(),
			server_names_source: "manual",
			fingerprint: "chrome",
		},
		managed_default: true,
		canary_upstream: fixtureCatalog.slotString.s3(),
		accepted_authorities: fixtureCatalog.slotList.l2(),
	};
}
