import type { AdminEndpointCreateRequest } from "../../src/api/adminEndpoints";
import type { AdminNode } from "../../src/api/adminNodes";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

function matchesFixtureValue(value: unknown, fixture: unknown): boolean {
	return JSON.stringify(value) === JSON.stringify(fixture);
}

function applyFixtureCanaryUpstream(
	meta: Record<string, unknown>,
	value: unknown,
): void {
	if (value === undefined || value === null) return;
	if (
		matchesFixtureValue(value, fixtureCatalog.canaryUpstream.httpsListener())
	) {
		meta.canary_upstream = fixtureCatalog.canaryUpstream.httpsListener();
		return;
	}
	if (
		matchesFixtureValue(value, fixtureCatalog.canaryUpstream.httpsAlternate())
	) {
		meta.canary_upstream = fixtureCatalog.canaryUpstream.httpsAlternate();
		return;
	}
	if (
		matchesFixtureValue(value, fixtureCatalog.canaryUpstream.httpLoopback())
	) {
		meta.canary_upstream = fixtureCatalog.canaryUpstream.httpLoopback();
		return;
	}
	throw new Error(
		"canary_upstream must be an approved synthetic fixture value",
	);
}

function applyFixtureAuthorities(
	meta: Record<string, unknown>,
	values: string[] | null | undefined,
): void {
	if (!values || values.length === 0) return;
	if (
		matchesFixtureValue(values, fixtureCatalog.authority.edgeExamplePort443())
	) {
		meta.accepted_authorities = fixtureCatalog.authority.edgeExamplePort443();
		return;
	}
	if (
		matchesFixtureValue(
			values,
			fixtureCatalog.authority.existingAuthoritiesPort443(),
		)
	) {
		meta.accepted_authorities =
			fixtureCatalog.authority.existingAuthoritiesPort443();
		return;
	}
	throw new Error(
		"accepted_authorities must be approved synthetic fixture values",
	);
}

export function buildEndpointCreateMeta(
	payload: AdminEndpointCreateRequest,
	nodes: AdminNode[],
): Record<string, unknown> {
	if (payload.kind !== fixtureCatalog.endpoint.vlessKind()) {
		return {};
	}

	if (payload.reality) {
		if (
			matchesFixtureValue(payload.reality, fixtureCatalog.endpoint.reality())
		) {
			return { reality: fixtureCatalog.endpoint.reality() };
		}
		if (
			matchesFixtureValue(
				payload.reality,
				fixtureCatalog.endpoint.realityAlternate(),
			)
		) {
			return { reality: fixtureCatalog.endpoint.realityAlternate() };
		}
		throw new Error("reality must be an approved synthetic fixture value");
	}

	const node = nodes.find((item) => item.node_id === payload.node_id);
	if (!node) throw new Error("node not found");
	const meta: Record<string, unknown> = {
		reality:
			node.node_id === fixtureCatalog.nodeId.fixture36()
				? fixtureCatalog.endpoint.realityAlternate()
				: fixtureCatalog.endpoint.reality(),
		managed_default: true,
	};
	applyFixtureCanaryUpstream(meta, payload.canary_upstream);
	applyFixtureAuthorities(meta, payload.accepted_authorities);
	return meta;
}
