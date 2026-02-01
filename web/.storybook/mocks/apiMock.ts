import type { AlertsResponse } from "../../src/api/adminAlerts";
import type {
	AdminEndpoint,
	AdminEndpointCreateRequest,
	AdminEndpointPatchRequest,
} from "../../src/api/adminEndpoints";
import type {
	AdminGrantGroupCreateRequest,
	AdminGrantGroupDetail,
	AdminGrantGroupReplaceRequest,
} from "../../src/api/adminGrantGroups";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminUserNodeQuota } from "../../src/api/adminUserNodeQuotas";
import type {
	AdminUser,
	AdminUserCreateRequest,
	AdminUserPatchRequest,
	AdminUserTokenResponse,
} from "../../src/api/adminUsers";
import type { ClusterInfoResponse } from "../../src/api/clusterInfo";
import type { GrantCredentials } from "../../src/api/grantCredentials";
import type { HealthResponse } from "../../src/api/health";
import type {
	NodeQuotaReset,
	QuotaResetSource,
	UserQuotaReset,
} from "../../src/api/quotaReset";
import type { VersionCheckResponse } from "../../src/api/versionCheck";

export type StorybookApiMockConfig = {
	adminToken?: string | null;
	data?: Partial<MockStateSeed>;
	failAdminConfig?: boolean;
	failVersionCheck?: boolean;
	failGrantGroupCreate?: boolean;
	delayGrantGroupCreateMs?: number;
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
	versionCheck: VersionCheckResponse;
	nodes: AdminNode[];
	endpoints: MockEndpointSeed[];
	users: AdminUser[];
	grantGroups: AdminGrantGroupDetail[];
	nodeQuotas: AdminUserNodeQuota[];
	alerts: AlertsResponse;
	subscriptions: Record<string, string>;
};

type MockState = Omit<MockStateSeed, "endpoints"> & {
	endpoints: MockEndpointRecord[];
	failAdminConfig: boolean;
	failVersionCheck: boolean;
	failGrantGroupCreate: boolean;
	delayGrantGroupCreateMs: number;
	grantGroupsByName: Record<string, AdminGrantGroupDetail>;
	counters: {
		endpoint: number;
		grantGroup: number;
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
	const defaultNodeQuotaReset = (dayOfMonth: number): NodeQuotaReset => ({
		policy: "monthly",
		day_of_month: dayOfMonth,
		tz_offset_minutes: null,
	});

	const defaultUserQuotaReset = (dayOfMonth: number): UserQuotaReset => ({
		policy: "monthly",
		day_of_month: dayOfMonth,
		tz_offset_minutes: 480,
	});

	const nodes: AdminNode[] = [
		{
			node_id: "node-1",
			node_name: "tokyo-1",
			api_base_url: "https://tokyo-1.example.com",
			access_host: "tokyo-1.example.com",
			quota_reset: defaultNodeQuotaReset(1),
		},
		{
			node_id: "node-2",
			node_name: "osaka-1",
			api_base_url: "https://osaka-1.example.com",
			access_host: "osaka-1.example.com",
			quota_reset: defaultNodeQuotaReset(15),
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
			quota_reset: defaultUserQuotaReset(1),
		},
		{
			user_id: "user-2",
			display_name: "Bob",
			subscription_token: "sub-user-2",
			quota_reset: defaultUserQuotaReset(15),
		},
	];

	const grantGroups: AdminGrantGroupDetail[] = [
		{
			group: { group_name: "group-demo" },
			members: [
				{
					user_id: "user-1",
					endpoint_id: "endpoint-1",
					enabled: true,
					quota_limit_bytes: 10_000_000,
					note: "Priority",
					credentials: createGrantCredentials(endpoints[0], 1),
				},
				{
					user_id: "user-2",
					endpoint_id: "endpoint-2",
					enabled: true,
					quota_limit_bytes: 5_000_000,
					note: null,
					credentials: createGrantCredentials(endpoints[1], 2),
				},
			],
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
			xp_version: "0.0.0",
		},
		versionCheck: {
			current: { package: "0.0.0", release_tag: "v0.0.0" },
			latest: { release_tag: "v0.0.0", published_at: "2026-01-31T00:00:00Z" },
			has_update: false,
			checked_at: "2026-01-31T00:00:00Z",
			compare_reason: "semver",
			source: {
				kind: "github-releases",
				repo: "IvanLi-CN/xp",
				api_base: "https://api.github.com",
				channel: "stable",
			},
		},
		nodes,
		endpoints,
		users,
		grantGroups,
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
		versionCheck: overrides?.versionCheck ?? base.versionCheck,
		nodes: overrides?.nodes ?? base.nodes,
		endpoints: overrides?.endpoints ?? base.endpoints,
		users: overrides?.users ?? base.users,
		grantGroups: overrides?.grantGroups ?? base.grantGroups,
		nodeQuotas: overrides?.nodeQuotas ?? base.nodeQuotas,
		alerts: overrides?.alerts ?? base.alerts,
		subscriptions: {
			...base.subscriptions,
			...(overrides?.subscriptions ?? {}),
		},
	};

	const counters = {
		endpoint: 1,
		grantGroup: 1,
		joinToken: 1,
		shortId: 1,
		subscription: 1,
		user: 1,
	};

	const endpoints = merged.endpoints.map((endpoint) =>
		ensureEndpointRecord(endpoint, counters),
	);

	const grantGroupsByName: Record<string, AdminGrantGroupDetail> = {};
	for (const group of merged.grantGroups) {
		grantGroupsByName[group.group.group_name] = clone(group);
	}

	return {
		...clone(merged),
		endpoints,
		failAdminConfig: config?.failAdminConfig ?? false,
		failVersionCheck: config?.failVersionCheck ?? false,
		failGrantGroupCreate: config?.failGrantGroupCreate ?? false,
		delayGrantGroupCreateMs: config?.delayGrantGroupCreateMs ?? 0,
		grantGroupsByName,
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

	if (path === "/api/version/check" && method === "GET") {
		if (state.failVersionCheck) {
			return errorResponse(502, "upstream_error", "mock version check failure");
		}
		return jsonResponse(clone(state.versionCheck));
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
		const payload = await readJson<{
			quota_limit_bytes?: number;
			quota_reset_source?: QuotaResetSource;
		}>(req);
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
			quota_reset_source: payload.quota_reset_source ?? "user",
		};

		state.nodeQuotas = [
			...state.nodeQuotas.filter(
				(q) => !(q.user_id === userId && q.node_id === nodeId),
			),
			updated,
		];

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
				quota_reset?: NodeQuotaReset;
			}>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const updated: AdminNode = {
				...node,
				node_name: payload.node_name ?? node.node_name,
				access_host: payload.access_host ?? node.access_host,
				api_base_url: payload.api_base_url ?? node.api_base_url,
				quota_reset: payload.quota_reset ?? node.quota_reset,
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
			quota_reset:
				payload.quota_reset ??
				({
					policy: "monthly",
					day_of_month: 1,
					tz_offset_minutes: 480,
				} satisfies UserQuotaReset),
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
				quota_reset: payload.quota_reset ?? user.quota_reset,
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

	if (path === "/api/admin/grant-groups" && method === "GET") {
		const items = Object.values(state.grantGroupsByName).map((detail) => ({
			group_name: detail.group.group_name,
			member_count: detail.members.length,
		}));
		items.sort((a, b) => a.group_name.localeCompare(b.group_name));
		return jsonResponse({ items });
	}

	if (path === "/api/admin/grant-groups" && method === "POST") {
		if (state.delayGrantGroupCreateMs > 0) {
			await new Promise((resolve) =>
				setTimeout(resolve, state.delayGrantGroupCreateMs),
			);
		}

		if (state.failGrantGroupCreate) {
			return errorResponse(409, "conflict", "group_name already exists");
		}

		const payload = await readJson<AdminGrantGroupCreateRequest>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		if (!payload.group_name) {
			return errorResponse(400, "invalid_request", "group_name is required");
		}
		if (!payload.members || payload.members.length === 0) {
			return errorResponse(
				400,
				"invalid_request",
				"members must have at least 1 item",
			);
		}
		if (state.grantGroupsByName[payload.group_name]) {
			return errorResponse(409, "conflict", "group_name already exists");
		}

		const endpointsById = new Map(
			state.endpoints.map(({ active_short_id, short_ids, ...rest }) => [
				rest.endpoint_id,
				rest,
			]),
		);

		const detail: AdminGrantGroupDetail = {
			group: { group_name: payload.group_name },
			members: payload.members.map((m) => {
				const endpoint = endpointsById.get(m.endpoint_id);
				return {
					user_id: m.user_id,
					endpoint_id: m.endpoint_id,
					enabled: m.enabled,
					quota_limit_bytes: Math.floor(m.quota_limit_bytes),
					note: m.note ?? null,
					credentials: createGrantCredentials(
						endpoint,
						state.counters.grantGroup++,
					),
				};
			}),
		};

		state.grantGroupsByName[payload.group_name] = clone(detail);
		return jsonResponse(detail, { status: 201 });
	}

	const grantGroupGetMatch = path.match(
		/^\/api\/admin\/grant-groups\/([^/]+)$/,
	);
	if (grantGroupGetMatch) {
		const groupName = decodeURIComponent(grantGroupGetMatch[1]);
		const existing = state.grantGroupsByName[groupName];
		if (!existing) {
			return errorResponse(404, "not_found", "grant group not found");
		}
		if (method === "GET") {
			return jsonResponse(clone(existing));
		}
		if (method === "PUT") {
			const payload = await readJson<AdminGrantGroupReplaceRequest>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			if (!payload.members || payload.members.length === 0) {
				return errorResponse(
					400,
					"invalid_request",
					"members must have at least 1 item",
				);
			}

			const renameTo = payload.rename_to?.trim() || undefined;
			const nextGroupName = renameTo ?? groupName;
			if (renameTo && state.grantGroupsByName[renameTo]) {
				return errorResponse(409, "conflict", "group_name already exists");
			}

			const oldByKey = new Map(
				existing.members.map((m) => [`${m.user_id}:${m.endpoint_id}`, m]),
			);
			const newByKey = new Map(
				payload.members.map((m) => [`${m.user_id}:${m.endpoint_id}`, m]),
			);

			let created = 0;
			let updated = 0;
			let deleted = 0;

			for (const key of oldByKey.keys()) {
				if (!newByKey.has(key)) deleted += 1;
			}
			for (const [key, next] of newByKey.entries()) {
				const prev = oldByKey.get(key);
				if (!prev) {
					created += 1;
					continue;
				}
				const nextNote = Object.prototype.hasOwnProperty.call(next, "note")
					? (next.note ?? null)
					: prev.note;
				if (
					prev.enabled !== next.enabled ||
					prev.quota_limit_bytes !== Math.floor(next.quota_limit_bytes) ||
					prev.note !== nextNote
				) {
					updated += 1;
				}
			}

			const endpointsById = new Map(
				state.endpoints.map(({ active_short_id, short_ids, ...rest }) => [
					rest.endpoint_id,
					rest,
				]),
			);

			const nextDetail: AdminGrantGroupDetail = {
				group: { group_name: nextGroupName },
				members: payload.members.map((m) => {
					const key = `${m.user_id}:${m.endpoint_id}`;
					const prev = oldByKey.get(key);
					const endpoint = endpointsById.get(m.endpoint_id);
					const note = Object.prototype.hasOwnProperty.call(m, "note")
						? (m.note ?? null)
						: (prev?.note ?? null);
					return {
						user_id: m.user_id,
						endpoint_id: m.endpoint_id,
						enabled: m.enabled,
						quota_limit_bytes: Math.floor(m.quota_limit_bytes),
						note,
						credentials:
							prev?.credentials ??
							createGrantCredentials(endpoint, state.counters.grantGroup++),
					};
				}),
			};

			if (nextGroupName !== groupName) {
				delete state.grantGroupsByName[groupName];
			}
			state.grantGroupsByName[nextGroupName] = clone(nextDetail);

			return jsonResponse({
				group: { group_name: nextGroupName },
				created,
				updated,
				deleted,
			});
		}
		if (method === "DELETE") {
			const memberCount = existing.members.length;
			delete state.grantGroupsByName[groupName];
			return jsonResponse({ deleted: memberCount });
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
	const key = JSON.stringify({ storyId, config: config ?? null });
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
