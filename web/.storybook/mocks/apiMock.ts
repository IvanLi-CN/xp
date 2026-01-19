import type { AlertsResponse } from "../../src/api/adminAlerts";
import type {
	AdminEndpoint,
	AdminEndpointCreateRequest,
	AdminEndpointPatchRequest,
} from "../../src/api/adminEndpoints";
import type {
	AdminGrant,
	AdminGrantCreateRequest,
	AdminGrantPatchRequest,
	AdminGrantUsageResponse,
	GrantCredentials,
} from "../../src/api/adminGrants";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminUserNodeQuota } from "../../src/api/adminUserNodeQuotas";
import type {
	AdminUser,
	AdminUserCreateRequest,
	AdminUserPatchRequest,
	AdminUserTokenResponse,
} from "../../src/api/adminUsers";
import type { ClusterInfoResponse } from "../../src/api/clusterInfo";
import type { HealthResponse } from "../../src/api/health";

export type StorybookApiMockConfig = {
	adminToken?: string | null;
	data?: Partial<MockStateSeed>;
	failAdminConfig?: boolean;
};

type MockEndpointSeed = AdminEndpoint & {
	active_short_id?: string;
	short_ids?: string[];
};

type MockEndpointRecord = AdminEndpoint & {
	active_short_id: string;
	short_ids: string[];
};

type MockStateSeed = {
	health: HealthResponse;
	clusterInfo: ClusterInfoResponse;
	nodes: AdminNode[];
	endpoints: MockEndpointSeed[];
	users: AdminUser[];
	grants: AdminGrant[];
	nodeQuotas: AdminUserNodeQuota[];
	alerts: AlertsResponse;
	subscriptions: Record<string, string>;
};

type MockState = Omit<MockStateSeed, "endpoints"> & {
	endpoints: MockEndpointRecord[];
	failAdminConfig: boolean;
	counters: {
		endpoint: number;
		grant: number;
		joinToken: number;
		shortId: number;
		subscription: number;
		user: number;
	};
};

type MockApi = {
	reset: (config?: StorybookApiMockConfig) => void;
	handle: (req: Request) => Promise<Response>;
};

let singletonMock: MockApi | null = null;
let lastStoryKey = "";
let fetchInstalled = false;
let originalFetch: typeof fetch | null = null;

const JSON_HEADERS = { "Content-Type": "application/json" } as const;
const TEXT_HEADERS = { "Content-Type": "text/plain" } as const;

function clone<T>(value: T): T {
	if (typeof structuredClone === "function") {
		return structuredClone(value);
	}
	return JSON.parse(JSON.stringify(value)) as T;
}

function jsonResponse(
	data: unknown,
	init?: { status?: number; headers?: Record<string, string> },
): Response {
	return new Response(JSON.stringify(data), {
		status: init?.status ?? 200,
		headers: { ...JSON_HEADERS, ...init?.headers },
	});
}

function textResponse(
	data: string,
	init?: { status?: number; headers?: Record<string, string> },
): Response {
	return new Response(data, {
		status: init?.status ?? 200,
		headers: { ...TEXT_HEADERS, ...init?.headers },
	});
}

function errorResponse(
	status: number,
	code: string,
	message: string,
	details: Record<string, unknown> = {},
): Response {
	return jsonResponse(
		{
			error: {
				code,
				message,
				details,
			},
		},
		{ status },
	);
}

function ensureEndpointRecord(
	seed: MockEndpointSeed,
	counters: MockState["counters"],
): MockEndpointRecord {
	const shortIds = seed.short_ids?.length
		? [...seed.short_ids]
		: [`short-${counters.shortId++}`, `short-${counters.shortId++}`];
	const activeShortId = seed.active_short_id ?? shortIds[0];
	return {
		...seed,
		short_ids: shortIds,
		active_short_id: activeShortId,
	};
}

function createDefaultSeed(): MockStateSeed {
	const nodes: AdminNode[] = [
		{
			node_id: "node-1",
			node_name: "tokyo-1",
			api_base_url: "https://tokyo-1.example.com",
			access_host: "tokyo-1.example.com",
		},
		{
			node_id: "node-2",
			node_name: "osaka-1",
			api_base_url: "https://osaka-1.example.com",
			access_host: "osaka-1.example.com",
		},
	];

	const endpoints: MockEndpointSeed[] = [
		{
			endpoint_id: "endpoint-1",
			node_id: "node-1",
			tag: "edge-tokyo",
			kind: "vless_reality_vision_tcp",
			port: 443,
			meta: {
				public_domain: "edge.tokyo.example.com",
				reality: {
					dest: "www.example.com:443",
					server_names: ["example.com", "www.example.com"],
					fingerprint: "chrome",
				},
			},
			short_ids: ["2a3b4c", "5d6e7f"],
			active_short_id: "2a3b4c",
		},
		{
			endpoint_id: "endpoint-2",
			node_id: "node-2",
			tag: "shadow-osaka",
			kind: "ss2022_2022_blake3_aes_128_gcm",
			port: 8443,
			meta: {
				method: "2022-blake3-aes-128-gcm",
			},
			short_ids: ["aa11bb"],
			active_short_id: "aa11bb",
		},
	];

	const users: AdminUser[] = [
		{
			user_id: "user-1",
			display_name: "Alice",
			subscription_token: "sub-user-1",
			cycle_policy_default: "by_user",
			cycle_day_of_month_default: 1,
		},
		{
			user_id: "user-2",
			display_name: "Bob",
			subscription_token: "sub-user-2",
			cycle_policy_default: "by_node",
			cycle_day_of_month_default: 15,
		},
	];

	const grants: AdminGrant[] = [
		{
			grant_id: "grant-1",
			user_id: "user-1",
			endpoint_id: "endpoint-1",
			enabled: true,
			quota_limit_bytes: 10_000_000,
			cycle_policy: "inherit_user",
			cycle_day_of_month: null,
			note: "Priority",
			credentials: {
				vless: {
					uuid: "11111111-1111-1111-1111-111111111111",
					email: "alice@example.com",
				},
			},
		},
		{
			grant_id: "grant-2",
			user_id: "user-2",
			endpoint_id: "endpoint-2",
			enabled: true,
			quota_limit_bytes: 5_000_000,
			cycle_policy: "by_user",
			cycle_day_of_month: 15,
			note: null,
			credentials: {
				ss2022: {
					method: "2022-blake3-aes-128-gcm",
					password: "mock-password",
				},
			},
		},
	];

	const alerts: AlertsResponse = {
		partial: false,
		unreachable_nodes: [],
		items: [
			{
				type: "quota_warning",
				grant_id: "grant-1",
				endpoint_id: "endpoint-1",
				owner_node_id: "node-1",
				desired_enabled: true,
				quota_banned: false,
				quota_banned_at: null,
				effective_enabled: true,
				message: "Usage is near the quota limit.",
				action_hint: "Consider raising the quota.",
			},
		],
	};

	const subscriptions: Record<string, string> = {
		"sub-user-1": "# raw subscription for sub-user-1\nnode-1",
		"sub-user-2": "# raw subscription for sub-user-2\nnode-2",
	};

	return {
		health: { status: "ok" },
		clusterInfo: {
			cluster_id: "cluster-alpha",
			node_id: "node-1",
			role: "leader",
			leader_api_base_url: "https://tokyo-1.example.com",
			term: 12,
		},
		nodes,
		endpoints,
		users,
		grants,
		nodeQuotas: [],
		alerts,
		subscriptions,
	};
}

function buildState(config?: StorybookApiMockConfig): MockState {
	const base = createDefaultSeed();
	const overrides = config?.data;

	const merged: MockStateSeed = {
		health: overrides?.health ?? base.health,
		clusterInfo: overrides?.clusterInfo ?? base.clusterInfo,
		nodes: overrides?.nodes ?? base.nodes,
		endpoints: overrides?.endpoints ?? base.endpoints,
		users: overrides?.users ?? base.users,
		grants: overrides?.grants ?? base.grants,
		nodeQuotas: overrides?.nodeQuotas ?? base.nodeQuotas,
		alerts: overrides?.alerts ?? base.alerts,
		subscriptions: {
			...base.subscriptions,
			...(overrides?.subscriptions ?? {}),
		},
	};

	const counters = {
		endpoint: 1,
		grant: 1,
		joinToken: 1,
		shortId: 1,
		subscription: 1,
		user: 1,
	};

	const endpoints = merged.endpoints.map((endpoint) =>
		ensureEndpointRecord(endpoint, counters),
	);

	return {
		...clone(merged),
		endpoints,
		failAdminConfig: config?.failAdminConfig ?? false,
		counters,
	};
}

function createGrantCredentials(
	endpoint: AdminEndpoint | undefined,
	counter: number,
): GrantCredentials {
	if (!endpoint || endpoint.kind === "vless_reality_vision_tcp") {
		return {
			vless: {
				uuid: `22222222-2222-2222-2222-${String(counter).padStart(12, "0")}`,
				email: `user${counter}@example.com`,
			},
		};
	}

	return {
		ss2022: {
			method: "2022-blake3-aes-128-gcm",
			password: `mock-password-${counter}`,
		},
	};
}

function buildSubscriptionText(token: string, format: string | null): string {
	if (format === "clash") {
		return `# clash subscription for ${token}\nproxy: mock-${token}`;
	}
	return `# raw subscription for ${token}\nproxy: mock-${token}`;
}

async function readJson<T>(req: Request): Promise<T | undefined> {
	const text = await req.text();
	if (!text) return undefined;
	try {
		return JSON.parse(text) as T;
	} catch {
		return undefined;
	}
}

async function handleRequest(
	state: MockState,
	req: Request,
): Promise<Response> {
	const method = req.method.toUpperCase();
	const url = new URL(req.url, "http://localhost");
	const path = url.pathname;

	if (!path.startsWith("/api/")) {
		return errorResponse(404, "not_found", "mock only handles /api/* requests");
	}

	if (path === "/api/health" && method === "GET") {
		return jsonResponse(state.health);
	}

	if (path === "/api/cluster/info" && method === "GET") {
		return jsonResponse(state.clusterInfo);
	}

	if (path === "/api/admin/config" && method === "GET") {
		if (state.failAdminConfig) {
			return errorResponse(500, "internal", "mock admin config failure");
		}
		const node = state.nodes[0];
		const token = "storybook-admin-token";
		return jsonResponse({
			bind: "127.0.0.1:62416",
			xray_api_addr: "127.0.0.1:10085",
			data_dir: "./data",
			node_name: node?.node_name ?? "node-1",
			access_host: node?.access_host ?? "",
			api_base_url: node?.api_base_url ?? "https://127.0.0.1:62416",
			quota_poll_interval_secs: 10,
			quota_auto_unban: true,
			admin_token_present: true,
			admin_token_masked: "*".repeat(token.length),
		});
	}

	if (path === "/api/admin/nodes" && method === "GET") {
		return jsonResponse({ items: clone(state.nodes) });
	}

	const userNodeQuotasMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-quotas$/,
	);
	if (userNodeQuotasMatch && method === "GET") {
		const userId = decodeURIComponent(userNodeQuotasMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const items = state.nodeQuotas.filter((q) => q.user_id === userId);
		return jsonResponse({ items: clone(items) });
	}

	const userNodeQuotaPutMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-quotas\/([^/]+)$/,
	);
	if (userNodeQuotaPutMatch && method === "PUT") {
		const userId = decodeURIComponent(userNodeQuotaPutMatch[1]);
		const nodeId = decodeURIComponent(userNodeQuotaPutMatch[2]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const nodeExists = state.nodes.some((n) => n.node_id === nodeId);
		if (!nodeExists) {
			return errorResponse(404, "not_found", "node not found");
		}
		const payload = await readJson<{ quota_limit_bytes?: number }>(req);
		if (!payload || typeof payload.quota_limit_bytes !== "number") {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		if (payload.quota_limit_bytes < 0) {
			return errorResponse(
				400,
				"invalid_request",
				"quota_limit_bytes must be non-negative",
			);
		}
		const quota = Math.floor(payload.quota_limit_bytes);
		const updated: AdminUserNodeQuota = {
			user_id: userId,
			node_id: nodeId,
			quota_limit_bytes: quota,
		};

		state.nodeQuotas = [
			...state.nodeQuotas.filter(
				(q) => !(q.user_id === userId && q.node_id === nodeId),
			),
			updated,
		];

		// Best-effort unification for existing grants on the node.
		const endpointsById = new Map(
			state.endpoints.map((ep) => [ep.endpoint_id, ep]),
		);
		state.grants = state.grants.map((g) => {
			if (g.user_id !== userId) return g;
			const ep = endpointsById.get(g.endpoint_id);
			if (!ep || ep.node_id !== nodeId) return g;
			return { ...g, quota_limit_bytes: quota };
		});

		return jsonResponse(clone(updated));
	}

	const nodeMatch = path.match(/^\/api\/admin\/nodes\/([^/]+)$/);
	if (nodeMatch) {
		const nodeId = decodeURIComponent(nodeMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		if (!node) {
			return errorResponse(404, "not_found", "node not found");
		}
		if (method === "GET") {
			return jsonResponse(clone(node));
		}
		if (method === "PATCH") {
			const payload = await readJson<{
				node_name?: string;
				access_host?: string;
				api_base_url?: string;
			}>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const updated: AdminNode = {
				...node,
				node_name: payload.node_name ?? node.node_name,
				access_host: payload.access_host ?? node.access_host,
				api_base_url: payload.api_base_url ?? node.api_base_url,
			};
			state.nodes = state.nodes.map((item) =>
				item.node_id === nodeId ? updated : item,
			);
			return jsonResponse(clone(updated));
		}
	}

	if (path === "/api/admin/cluster/join-tokens" && method === "POST") {
		const payload = await readJson<{ ttl_seconds?: number }>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		const ttl = payload.ttl_seconds ?? 0;
		const joinToken = `join-mock-${state.counters.joinToken++}-${ttl}`;
		return jsonResponse({ join_token: joinToken });
	}

	if (path === "/api/admin/endpoints" && method === "GET") {
		return jsonResponse({
			items: state.endpoints.map(({ active_short_id, short_ids, ...rest }) =>
				clone(rest),
			),
		});
	}

	if (path === "/api/admin/endpoints" && method === "POST") {
		const payload = await readJson<AdminEndpointCreateRequest>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		if (!payload.node_id || !payload.kind || !payload.port) {
			return errorResponse(
				400,
				"invalid_request",
				"missing required endpoint fields",
			);
		}
		const endpointId = `endpoint-mock-${state.counters.endpoint++}`;
		const tag = `${payload.kind}-${endpointId}`;
		let meta: Record<string, unknown> = {};
		if (payload.kind === "vless_reality_vision_tcp") {
			if (!payload.reality) {
				return errorResponse(
					400,
					"invalid_request",
					"missing vless reality fields",
				);
			}
			meta = {
				reality: payload.reality,
			};
		}
		const endpoint: AdminEndpoint = {
			endpoint_id: endpointId,
			node_id: payload.node_id,
			tag,
			kind: payload.kind,
			port: payload.port,
			meta,
		};
		const record: MockEndpointRecord = {
			...endpoint,
			short_ids: [`short-${state.counters.shortId++}`],
			active_short_id: `short-${state.counters.shortId++}`,
		};
		record.short_ids.unshift(record.active_short_id);
		state.endpoints = [...state.endpoints, record];
		return jsonResponse(endpoint);
	}

	const endpointRotateMatch = path.match(
		/^\/api\/admin\/endpoints\/([^/]+)\/rotate-shortid$/,
	);
	if (endpointRotateMatch && method === "POST") {
		const endpointId = decodeURIComponent(endpointRotateMatch[1]);
		const endpoint = state.endpoints.find(
			(item) => item.endpoint_id === endpointId,
		);
		if (!endpoint) {
			return errorResponse(404, "not_found", "endpoint not found");
		}
		const nextShortId = `short-${state.counters.shortId++}`;
		endpoint.active_short_id = nextShortId;
		endpoint.short_ids = [nextShortId, ...endpoint.short_ids].slice(0, 5);
		return jsonResponse({
			endpoint_id: endpoint.endpoint_id,
			active_short_id: endpoint.active_short_id,
			short_ids: clone(endpoint.short_ids),
		});
	}

	const endpointMatch = path.match(/^\/api\/admin\/endpoints\/([^/]+)$/);
	if (endpointMatch) {
		const endpointId = decodeURIComponent(endpointMatch[1]);
		const endpoint = state.endpoints.find(
			(item) => item.endpoint_id === endpointId,
		);
		if (!endpoint) {
			return errorResponse(404, "not_found", "endpoint not found");
		}
		if (method === "GET") {
			const { active_short_id, short_ids, ...rest } = endpoint;
			return jsonResponse(clone(rest));
		}
		if (method === "PATCH") {
			const payload = await readJson<AdminEndpointPatchRequest>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const nextMeta = { ...endpoint.meta } as Record<string, unknown>;
			if (payload.reality !== undefined) {
				nextMeta.reality = payload.reality;
			}
			const updated: MockEndpointRecord = {
				...endpoint,
				port: payload.port ?? endpoint.port,
				meta: nextMeta,
			};
			state.endpoints = state.endpoints.map((item) =>
				item.endpoint_id === endpointId ? updated : item,
			);
			const { active_short_id, short_ids, ...rest } = updated;
			return jsonResponse(clone(rest));
		}
		if (method === "DELETE") {
			state.endpoints = state.endpoints.filter(
				(item) => item.endpoint_id !== endpointId,
			);
			return new Response(null, { status: 204 });
		}
	}

	if (path === "/api/admin/users" && method === "GET") {
		return jsonResponse({ items: clone(state.users) });
	}

	if (path === "/api/admin/users" && method === "POST") {
		const payload = await readJson<AdminUserCreateRequest>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		const userId = `user-mock-${state.counters.user++}`;
		const token = `sub-mock-${state.counters.subscription++}`;
		const user: AdminUser = {
			user_id: userId,
			display_name: payload.display_name,
			subscription_token: token,
			cycle_policy_default: payload.cycle_policy_default,
			cycle_day_of_month_default: payload.cycle_day_of_month_default,
		};
		state.users = [...state.users, user];
		state.subscriptions[token] = buildSubscriptionText(token, null);
		return jsonResponse(user);
	}

	const userMatch = path.match(/^\/api\/admin\/users\/([^/]+)$/);
	if (userMatch) {
		const userId = decodeURIComponent(userMatch[1]);
		const user = state.users.find((item) => item.user_id === userId);
		if (!user) {
			return errorResponse(404, "not_found", "user not found");
		}
		if (method === "GET") {
			return jsonResponse(clone(user));
		}
		if (method === "PATCH") {
			const payload = await readJson<AdminUserPatchRequest>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const updated: AdminUser = {
				...user,
				display_name: payload.display_name ?? user.display_name,
				cycle_policy_default:
					payload.cycle_policy_default ?? user.cycle_policy_default,
				cycle_day_of_month_default:
					payload.cycle_day_of_month_default ?? user.cycle_day_of_month_default,
			};
			state.users = state.users.map((item) =>
				item.user_id === userId ? updated : item,
			);
			return jsonResponse(clone(updated));
		}
		if (method === "DELETE") {
			state.users = state.users.filter((item) => item.user_id !== userId);
			return new Response(null, { status: 204 });
		}
	}

	const userResetMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/reset-token$/,
	);
	if (userResetMatch && method === "POST") {
		const userId = decodeURIComponent(userResetMatch[1]);
		const user = state.users.find((item) => item.user_id === userId);
		if (!user) {
			return errorResponse(404, "not_found", "user not found");
		}
		const token = `sub-mock-${state.counters.subscription++}`;
		const updated: AdminUser = {
			...user,
			subscription_token: token,
		};
		state.users = state.users.map((item) =>
			item.user_id === userId ? updated : item,
		);
		state.subscriptions[token] = buildSubscriptionText(token, null);
		const response: AdminUserTokenResponse = { subscription_token: token };
		return jsonResponse(response);
	}

	if (path === "/api/admin/grants" && method === "GET") {
		return jsonResponse({ items: clone(state.grants) });
	}

	if (path === "/api/admin/grants" && method === "POST") {
		const payload = await readJson<AdminGrantCreateRequest>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		if (!payload.user_id || !payload.endpoint_id) {
			return errorResponse(400, "invalid_request", "missing grant identifiers");
		}
		const endpoint = state.endpoints.find(
			(item) => item.endpoint_id === payload.endpoint_id,
		);
		const grantId = `grant-mock-${state.counters.grant++}`;
		const grant: AdminGrant = {
			grant_id: grantId,
			user_id: payload.user_id,
			endpoint_id: payload.endpoint_id,
			enabled: true,
			quota_limit_bytes: payload.quota_limit_bytes,
			cycle_policy: payload.cycle_policy,
			cycle_day_of_month: payload.cycle_day_of_month,
			note: payload.note ?? null,
			credentials: createGrantCredentials(endpoint, state.counters.grant),
		};
		state.grants = [...state.grants, grant];
		return jsonResponse(grant);
	}

	const grantUsageMatch = path.match(/^\/api\/admin\/grants\/([^/]+)\/usage$/);
	if (grantUsageMatch && method === "GET") {
		const grantId = decodeURIComponent(grantUsageMatch[1]);
		const grant = state.grants.find((item) => item.grant_id === grantId);
		if (!grant) {
			return errorResponse(404, "not_found", "grant not found");
		}
		const usedBytes = Math.min(
			Math.round(grant.quota_limit_bytes * 0.35),
			grant.quota_limit_bytes,
		);
		const response: AdminGrantUsageResponse = {
			grant_id: grant.grant_id,
			cycle_start_at: "2024-12-01T00:00:00Z",
			cycle_end_at: "2025-01-01T00:00:00Z",
			used_bytes: usedBytes,
			owner_node_id: state.nodes[0]?.node_id ?? "node-1",
			desired_enabled: grant.enabled,
			quota_banned: false,
			quota_banned_at: null,
			effective_enabled: grant.enabled,
			warning:
				usedBytes > grant.quota_limit_bytes * 0.8
					? "quota nearly exhausted"
					: null,
		};
		return jsonResponse(response);
	}

	const grantMatch = path.match(/^\/api\/admin\/grants\/([^/]+)$/);
	if (grantMatch) {
		const grantId = decodeURIComponent(grantMatch[1]);
		const grant = state.grants.find((item) => item.grant_id === grantId);
		if (!grant) {
			return errorResponse(404, "not_found", "grant not found");
		}
		if (method === "GET") {
			return jsonResponse(clone(grant));
		}
		if (method === "PATCH") {
			const payload = await readJson<AdminGrantPatchRequest>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const hasNote = Object.prototype.hasOwnProperty.call(payload, "note");
			const nextNote = hasNote
				? payload.note === null
					? null
					: typeof payload.note === "string"
						? payload.note
						: grant.note
				: grant.note;
			const updated: AdminGrant = {
				...grant,
				enabled: payload.enabled,
				quota_limit_bytes: payload.quota_limit_bytes,
				cycle_policy: payload.cycle_policy,
				cycle_day_of_month: payload.cycle_day_of_month,
				note: nextNote,
			};
			state.grants = state.grants.map((item) =>
				item.grant_id === grantId ? updated : item,
			);
			return jsonResponse(clone(updated));
		}
		if (method === "DELETE") {
			state.grants = state.grants.filter((item) => item.grant_id !== grantId);
			return new Response(null, { status: 204 });
		}
	}

	if (path === "/api/admin/alerts" && method === "GET") {
		return jsonResponse(clone(state.alerts));
	}

	if (path.startsWith("/api/sub/") && method === "GET") {
		const token = decodeURIComponent(path.replace("/api/sub/", ""));
		const format = url.searchParams.get("format");
		const content =
			state.subscriptions[token] ?? buildSubscriptionText(token, format);
		return textResponse(content);
	}

	return errorResponse(404, "not_found", `no mock for ${method} ${path}`);
}

export function createMockApi(config?: StorybookApiMockConfig): MockApi {
	let state = buildState(config);
	return {
		reset(nextConfig?: StorybookApiMockConfig) {
			state = buildState(nextConfig);
		},
		async handle(req: Request) {
			return handleRequest(state, req);
		},
	};
}

export function configureStorybookApiMock(
	storyId: string,
	config?: StorybookApiMockConfig,
): void {
	const key = JSON.stringify({ storyId, data: config?.data ?? null });
	if (key === lastStoryKey) return;
	if (!singletonMock) {
		singletonMock = createMockApi(config);
	} else {
		singletonMock.reset(config);
	}
	lastStoryKey = key;
}

export function installStorybookFetchMock(): void {
	if (fetchInstalled) return;
	if (!globalThis.fetch) {
		throw new Error("fetch is not available to install Storybook mock");
	}
	originalFetch = globalThis.fetch.bind(globalThis);
	if (!singletonMock) {
		singletonMock = createMockApi();
	}
	globalThis.fetch = async (input, init) => {
		const request = input instanceof Request ? input : new Request(input, init);
		const url = new URL(
			request.url,
			globalThis.location?.origin ?? "http://localhost",
		);
		if (url.pathname.startsWith("/api/")) {
			const mock = singletonMock;
			if (!mock) {
				return errorResponse(500, "mock_unavailable", "mock not initialized");
			}
			return mock.handle(request);
		}
		if (!originalFetch) {
			return errorResponse(500, "mock_unavailable", "original fetch missing");
		}
		return originalFetch(input as RequestInfo, init);
	};
	fetchInstalled = true;
}
