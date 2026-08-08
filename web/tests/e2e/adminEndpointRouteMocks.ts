import type { Route } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

const DEFAULT_VLESS_CANARY_BIND = "127.0.0.1:39043";

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
	adminConfigVlessCanaryBind?: string;
	nodes: AdminNodeLike[];
	endpoints: AdminEndpointLike[];
};

type JsonRequest = {
	postData(): string | null;
};

type RouteContext = {
	adminConfigVlessCanaryBind?: string;
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
	adminConfigVlessCanaryBind,
	path,
	method,
	route,
	request,
	state,
}: RouteContext): boolean {
	if (path === "/api/admin/config" && method === "GET") {
		const managedVlessEndpoint = state.endpoints.find(
			(endpoint) =>
				endpoint.kind === "vless_reality_vision_tcp" &&
				endpoint.meta.managed_default === true,
		);
		const reality = managedVlessEndpoint?.meta.reality as
			| { dest?: unknown }
			| undefined;
		const vlessCanaryBind =
			typeof adminConfigVlessCanaryBind === "string"
				? adminConfigVlessCanaryBind
				: typeof reality?.dest === "string"
					? reality.dest
					: DEFAULT_VLESS_CANARY_BIND;
		jsonResponse(route, {
			bind: fixtureCatalog.slotString.s58(),
			xray_api_addr: fixtureCatalog.slotString.s59(),
			data_dir: "./data",
			node_name: fixtureCatalog.slotString.s74(),
			access_host: fixtureCatalog.slotString.s75(),
			api_base_url: fixtureCatalog.slotString.s76(),
			vless_https_canary_bind: vlessCanaryBind,
			quota_poll_interval_secs: 10,
			quota_auto_unban: true,
			ip_geo_enabled: false,
			ip_geo_origin: "https://api.country.is",
			mihomo_resource_allow_private_targets: false,
			admin_token_present: true,
			admin_token_masked: "********",
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
			endpoint_id: fixtureCatalog.slotString.s68(),
			node_id: fixtureCatalog.slotString.s77(),
			tag: fixtureCatalog.slotString.s78(),
			kind:
				payload.kind === "ss2022_2022_blake3_aes_128_gcm"
					? "ss2022_2022_blake3_aes_128_gcm"
					: "vless_reality_vision_tcp",
			port: typeof payload.port === "number" ? payload.port : 0,
			meta: {},
		};
		if (payload.reality && typeof payload.reality === "object") {
			newEndpoint.meta.reality = payload.reality;
		}
		if (
			payload.canary_upstream &&
			typeof payload.canary_upstream === "object"
		) {
			newEndpoint.meta.canary_upstream = payload.canary_upstream;
		}
		if (Array.isArray(payload.accepted_authorities)) {
			newEndpoint.meta.accepted_authorities = payload.accepted_authorities;
		}
		if (
			newEndpoint.kind === "vless_reality_vision_tcp" &&
			!("managed_default" in newEndpoint.meta)
		) {
			newEndpoint.meta.managed_default = !payload.reality;
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
		if (typeof payload.port === "number") {
			endpoint.port = payload.port;
		}
		if (payload.reality && typeof payload.reality === "object") {
			endpoint.meta.reality = payload.reality;
		}
		if (Object.prototype.hasOwnProperty.call(payload, "canary_upstream")) {
			if (payload.canary_upstream === null) {
				endpoint.meta.canary_upstream = fixtureCatalog.optional.undefined();
			} else if (
				payload.canary_upstream &&
				typeof payload.canary_upstream === "object"
			) {
				endpoint.meta.canary_upstream = payload.canary_upstream;
			}
		}
		if (Object.prototype.hasOwnProperty.call(payload, "accepted_authorities")) {
			if (payload.accepted_authorities === null) {
				endpoint.meta.accepted_authorities =
					fixtureCatalog.optional.undefined();
			} else if (Array.isArray(payload.accepted_authorities)) {
				endpoint.meta.accepted_authorities = payload.accepted_authorities;
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
