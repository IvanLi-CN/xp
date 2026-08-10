import type { Route } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

type AdminNodeLike = {
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
		const newEndpoint: AdminEndpointLike = {
			endpoint_id: fixtureCatalog.endpointId.fixture68(),
			node_id: fixtureCatalog.nodeId.fixture77(),
			tag: fixtureCatalog.endpointTag.fixture78(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port9443(),
			meta: { reality: fixtureCatalog.endpoint.reality() },
		};
		if (
			payload.canary_upstream &&
			typeof payload.canary_upstream === "object"
		) {
			newEndpoint.meta.canary_upstream =
				fixtureCatalog.canaryUpstream.httpsListener();
		}
		if (Array.isArray(payload.accepted_authorities)) {
			newEndpoint.meta.accepted_authorities =
				fixtureCatalog.authority.edgeExamplePort443();
		}
		if (
			newEndpoint.kind === fixtureCatalog.endpoint.vlessKind() &&
			!("managed_default" in newEndpoint.meta)
		) {
			newEndpoint.meta.managed_default = true;
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
		endpoint.port = fixtureCatalog.endpoint.port443();
		endpoint.meta.reality = fixtureCatalog.endpoint.reality();
		if (Object.prototype.hasOwnProperty.call(payload, "canary_upstream")) {
			if (payload.canary_upstream === null) {
				endpoint.meta.canary_upstream = fixtureCatalog.optional.undefined();
			} else if (
				payload.canary_upstream &&
				typeof payload.canary_upstream === "object"
			) {
				endpoint.meta.canary_upstream =
					fixtureCatalog.canaryUpstream.httpsListener();
			}
		}
		if (Object.prototype.hasOwnProperty.call(payload, "accepted_authorities")) {
			if (payload.accepted_authorities === null) {
				endpoint.meta.accepted_authorities =
					fixtureCatalog.optional.undefined();
			} else if (Array.isArray(payload.accepted_authorities)) {
				endpoint.meta.accepted_authorities =
					fixtureCatalog.authority.edgeExamplePort443();
			}
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
