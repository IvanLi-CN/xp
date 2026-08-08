import type { AdminEndpointCreateRequest } from "../../src/api/adminEndpoints";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminRealityDomain } from "../../src/api/adminRealityDomains";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import {
	normalizeAcceptedAuthority,
	validateAcceptedAuthority,
} from "../../src/utils/acceptedAuthority";

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

function normalizeAcceptedAuthoritiesForMock(values: string[]): string[] {
	const out: string[] = [];
	const seen = new Set<string>();
	for (const value of values) {
		const normalized = normalizeAcceptedAuthority(value);
		if (!normalized) continue;
		const err = validateAcceptedAuthority(normalized);
		if (err) throw new Error(err);
		if (seen.has(normalized)) continue;
		seen.add(normalized);
		out.push(normalized);
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
				dest: `${derived[0]}:443`,
				server_names: derived,
				server_names_source: source,
			},
		};
	}

	const node = nodes.find((item) => item.node_id === payload.node_id);
	if (!node) throw new Error("node not found");
	const acceptedAuthorities = normalizeAcceptedAuthoritiesForMock(
		payload.accepted_authorities ?? [],
	);

	return {
		reality: {
			dest: fixtureCatalog.address.loopback39043(),
			server_names: [node.access_host.replace(/\.$/, "")],
			server_names_source: "manual",
			fingerprint: "chrome",
		},
		managed_default: true,
		canary_upstream: payload.canary_upstream
			? {
					...payload.canary_upstream,
					url: payload.canary_upstream.url.trim(),
				}
			: undefined,
		accepted_authorities: acceptedAuthorities,
	};
}
