import type { Route } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

type AdminNodeLike = {
	node_id: string;
	node_name: string;
	access_host: string;
	api_base_url: string;
};

type AdminEndpointLike = {
	endpoint_id: string;
	node_id: string;
	tag: string;
	kind: "vless_reality_vision_tcp" | "ss2022_2022_blake3_aes_128_gcm";
	port: number;
	meta: Record<string, unknown>;
};

type ManagedEndpointState = {
	nodes: AdminNodeLike[];
	endpoints: AdminEndpointLike[];
	nextEndpointId(): string;
	nextEndpointTag(): string;
};

type JsonRequest = {
	postData(): string | null;
};

type RouteContext = {
	path: string;
	method: string;
	route: Route;
	request: JsonRequest;
	state: ManagedEndpointState;
};

function jsonResponse(route: Route, payload: unknown, status = 200): void {
	void route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function errorResponse(route: Route, message: string, status = 404): void {
	jsonResponse(
		route,
		{
			error: {
				code: "not_mocked",
				message,
				details: {},
			},
		},
		status,
	);
}

function parseJsonBody(request: JsonRequest): Record<string, unknown> {
	const raw = request.postData();
	if (!raw) {
		return {};
	}
	return JSON.parse(raw) as Record<string, unknown>;
}

function matchesFixtureValue(value: unknown, fixture: unknown): boolean {
	return JSON.stringify(value) === JSON.stringify(fixture);
}

function hasFixtureEndpointPort(value: unknown): boolean {
	return (
		value === fixtureCatalog.endpoint.port443() ||
		value === fixtureCatalog.endpoint.port444() ||
		value === fixtureCatalog.endpoint.port445() ||
		value === fixtureCatalog.endpoint.port8443() ||
		value === fixtureCatalog.endpoint.port8388() ||
		value === fixtureCatalog.endpoint.port9443() ||
		value === fixtureCatalog.endpoint.port53842() ||
		value === fixtureCatalog.endpoint.port53843() ||
		value === fixtureCatalog.endpoint.port53844()
	);
}

function hasFixtureEndpointNode(value: unknown): boolean {
	return (
		value === fixtureCatalog.identifier.nodePrimary() ||
		value === fixtureCatalog.identifier.nodeSecondary() ||
		value === fixtureCatalog.identifier.nodeTertiary() ||
		value === fixtureCatalog.nodeId.fixture32() ||
		value === fixtureCatalog.nodeId.fixture36() ||
		value === fixtureCatalog.nodeId.fixture63()
	);
}

function applyFixtureEndpointMetadata(
	endpoint: AdminEndpointLike,
	payload: Record<string, unknown>,
): boolean {
	if (payload.reality !== undefined) {
		if (
			matchesFixtureValue(payload.reality, fixtureCatalog.endpoint.reality())
		) {
			endpoint.meta.reality = fixtureCatalog.endpoint.reality();
		} else if (
			matchesFixtureValue(
				payload.reality,
				fixtureCatalog.endpoint.realityAlternate(),
			)
		) {
			endpoint.meta.reality = fixtureCatalog.endpoint.realityAlternate();
		} else {
			return false;
		}
	}
	if (payload.canary_upstream === null) {
		endpoint.meta.canary_upstream = fixtureCatalog.optional.undefined();
	}
	if (
		payload.canary_upstream !== undefined &&
		payload.canary_upstream !== null
	) {
		if (
			matchesFixtureValue(
				payload.canary_upstream,
				fixtureCatalog.canaryUpstream.httpsListener(),
			)
		) {
			endpoint.meta.canary_upstream =
				fixtureCatalog.canaryUpstream.httpsListener();
		} else if (
			matchesFixtureValue(
				payload.canary_upstream,
				fixtureCatalog.canaryUpstream.httpsAlternate(),
			)
		) {
			endpoint.meta.canary_upstream =
				fixtureCatalog.canaryUpstream.httpsAlternate();
		} else if (
			matchesFixtureValue(
				payload.canary_upstream,
				fixtureCatalog.canaryUpstream.httpLoopback(),
			)
		) {
			endpoint.meta.canary_upstream =
				fixtureCatalog.canaryUpstream.httpLoopback();
		} else {
			return false;
		}
	}
	if (payload.accepted_authorities === null) {
		endpoint.meta.accepted_authorities = fixtureCatalog.optional.undefined();
	}
	if (
		payload.accepted_authorities !== undefined &&
		payload.accepted_authorities !== null &&
		!Array.isArray(payload.accepted_authorities)
	) {
		return false;
	}
	if (Array.isArray(payload.accepted_authorities)) {
		if (
			matchesFixtureValue(
				payload.accepted_authorities,
				fixtureCatalog.authority.edgeExamplePort443(),
			)
		) {
			endpoint.meta.accepted_authorities =
				fixtureCatalog.authority.edgeExamplePort443();
		} else if (
			matchesFixtureValue(
				payload.accepted_authorities,
				fixtureCatalog.authority.existingAuthoritiesPort443(),
			)
		) {
			endpoint.meta.accepted_authorities =
				fixtureCatalog.authority.existingAuthoritiesPort443();
		} else {
			return false;
		}
	}
	return true;
}

export function handleAdminConfigAndEndpointRoutes({
	path,
	method,
	route,
	request,
	state,
}: RouteContext): boolean {
	if (path === "/api/admin/config" && method === "GET") {
		jsonResponse(route, {
			bind: fixtureCatalog.address.loopbackPort39058(),
			xray_api_addr: fixtureCatalog.address.loopbackPort39059(),
			data_dir: "./data",
			node_name: fixtureCatalog.nodeName.fixture74(),
			access_host: fixtureCatalog.host.fixture75(),
			api_base_url: fixtureCatalog.service.fixture76(),
			vless_https_canary_bind: fixtureCatalog.address.loopback39043(),
			quota_poll_interval_secs: 10,
			quota_auto_unban: true,
			ip_geo_enabled: false,
			ip_geo_origin: fixtureCatalog.url.publicOrigin(),
			mihomo_resource_allow_private_targets: false,
			admin_token_present: true,
			admin_token_masked: fixtureCatalog.subscription.providerPassword(),
		});
		return true;
	}

	if (path === "/api/admin/endpoints" && method === "GET") {
		jsonResponse(route, { items: state.endpoints });
		return true;
	}

	if (path === "/api/admin/endpoints" && method === "POST") {
		const payload = parseJsonBody(request);
		if (
			!hasFixtureEndpointNode(payload.node_id) ||
			!state.nodes.some((node) => node.node_id === payload.node_id) ||
			(payload.kind !== fixtureCatalog.endpoint.vlessKind() &&
				payload.kind !== fixtureCatalog.endpoint.ssKind()) ||
			!hasFixtureEndpointPort(payload.port)
		) {
			errorResponse(
				route,
				"endpoint fields must be approved fixture values",
				400,
			);
			return true;
		}
		const newEndpoint: AdminEndpointLike = {
			endpoint_id: state.nextEndpointId(),
			node_id: fixtureCatalog.nodeId.fixture32(),
			tag: state.nextEndpointTag(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
			meta: {},
		};
		if (payload.node_id === fixtureCatalog.identifier.nodePrimary()) {
			newEndpoint.node_id = fixtureCatalog.identifier.nodePrimary();
		} else if (payload.node_id === fixtureCatalog.identifier.nodeSecondary()) {
			newEndpoint.node_id = fixtureCatalog.identifier.nodeSecondary();
		} else if (payload.node_id === fixtureCatalog.identifier.nodeTertiary()) {
			newEndpoint.node_id = fixtureCatalog.identifier.nodeTertiary();
		} else if (payload.node_id === fixtureCatalog.nodeId.fixture36()) {
			newEndpoint.node_id = fixtureCatalog.nodeId.fixture36();
		} else if (payload.node_id === fixtureCatalog.nodeId.fixture63()) {
			newEndpoint.node_id = fixtureCatalog.nodeId.fixture63();
		}
		if (payload.kind === fixtureCatalog.endpoint.ssKind()) {
			newEndpoint.kind = fixtureCatalog.endpoint.ssKind();
		}
		if (payload.port === fixtureCatalog.endpoint.port444()) {
			newEndpoint.port = fixtureCatalog.endpoint.port444();
		} else if (payload.port === fixtureCatalog.endpoint.port445()) {
			newEndpoint.port = fixtureCatalog.endpoint.port445();
		} else if (payload.port === fixtureCatalog.endpoint.port8443()) {
			newEndpoint.port = fixtureCatalog.endpoint.port8443();
		} else if (payload.port === fixtureCatalog.endpoint.port8388()) {
			newEndpoint.port = fixtureCatalog.endpoint.port8388();
		} else if (payload.port === fixtureCatalog.endpoint.port9443()) {
			newEndpoint.port = fixtureCatalog.endpoint.port9443();
		} else if (payload.port === fixtureCatalog.endpoint.port53842()) {
			newEndpoint.port = fixtureCatalog.endpoint.port53842();
		} else if (payload.port === fixtureCatalog.endpoint.port53843()) {
			newEndpoint.port = fixtureCatalog.endpoint.port53843();
		} else if (payload.port === fixtureCatalog.endpoint.port53844()) {
			newEndpoint.port = fixtureCatalog.endpoint.port53844();
		}
		if (!applyFixtureEndpointMetadata(newEndpoint, payload)) {
			errorResponse(
				route,
				"endpoint metadata must be approved fixture values",
				400,
			);
			return true;
		}
		if (newEndpoint.kind === fixtureCatalog.endpoint.vlessKind()) {
			if (payload.reality === undefined) {
				if (!("reality" in newEndpoint.meta)) {
					newEndpoint.meta.reality = fixtureCatalog.endpoint.reality();
				}
				newEndpoint.meta.managed_default = true;
			} else {
				newEndpoint.meta.managed_default = false;
			}
		}
		state.endpoints.push(newEndpoint);
		jsonResponse(route, newEndpoint, 201);
		return true;
	}

	const endpointMatch = path.match(/^\/api\/admin\/endpoints\/([^/]+)$/);
	if (!endpointMatch) {
		return false;
	}

	const endpointId = decodeURIComponent(endpointMatch[1]);
	const endpoint = state.endpoints.find(
		(item) => item.endpoint_id === endpointId,
	);
	if (!endpoint) {
		errorResponse(route, `endpoint not found: ${endpointId}`, 404);
		return true;
	}

	if (method === "GET") {
		jsonResponse(route, endpoint);
		return true;
	}

	if (method === "PATCH") {
		const payload = parseJsonBody(request);
		if (payload.port !== undefined && !hasFixtureEndpointPort(payload.port)) {
			errorResponse(
				route,
				"endpoint port must be an approved fixture value",
				400,
			);
			return true;
		}
		if (payload.port === fixtureCatalog.endpoint.port444()) {
			endpoint.port = fixtureCatalog.endpoint.port444();
		} else if (payload.port === fixtureCatalog.endpoint.port445()) {
			endpoint.port = fixtureCatalog.endpoint.port445();
		} else if (payload.port === fixtureCatalog.endpoint.port8443()) {
			endpoint.port = fixtureCatalog.endpoint.port8443();
		} else if (payload.port === fixtureCatalog.endpoint.port8388()) {
			endpoint.port = fixtureCatalog.endpoint.port8388();
		} else if (payload.port === fixtureCatalog.endpoint.port9443()) {
			endpoint.port = fixtureCatalog.endpoint.port9443();
		} else if (payload.port === fixtureCatalog.endpoint.port53842()) {
			endpoint.port = fixtureCatalog.endpoint.port53842();
		} else if (payload.port === fixtureCatalog.endpoint.port53843()) {
			endpoint.port = fixtureCatalog.endpoint.port53843();
		} else if (payload.port === fixtureCatalog.endpoint.port53844()) {
			endpoint.port = fixtureCatalog.endpoint.port53844();
		} else if (payload.port === fixtureCatalog.endpoint.port443()) {
			endpoint.port = fixtureCatalog.endpoint.port443();
		}
		if (!applyFixtureEndpointMetadata(endpoint, payload)) {
			errorResponse(
				route,
				"endpoint metadata must be approved fixture values",
				400,
			);
			return true;
		}
		jsonResponse(route, endpoint);
		return true;
	}

	if (method === "DELETE") {
		state.endpoints = state.endpoints.filter(
			(item) => item.endpoint_id !== endpointId,
		);
		void route.fulfill({ status: 204, body: "" });
		return true;
	}

	return false;
}
