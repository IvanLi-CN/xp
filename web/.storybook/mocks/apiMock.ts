import type { AlertsResponse } from "../../src/api/adminAlerts";
import type {
	AdminEndpoint,
	AdminEndpointCreateRequest,
	AdminEndpointPatchRequest,
} from "../../src/api/adminEndpoints";
import type {
	AdminNodeRuntimeDetailResponse,
	AdminNodeRuntimeListItem,
	NodeRuntimeComponent,
	NodeRuntimeEvent,
	NodeRuntimeHistorySlot,
} from "../../src/api/adminNodeRuntime";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminQuotaPolicyGlobalWeightRow } from "../../src/api/adminQuotaPolicyGlobalWeightRows";
import type { AdminQuotaPolicyNodePolicy } from "../../src/api/adminQuotaPolicyNodePolicy";
import type { AdminQuotaPolicyNodeWeightRow } from "../../src/api/adminQuotaPolicyNodeWeightRows";
import type { AdminRealityDomain } from "../../src/api/adminRealityDomains";
import type {
	AdminUserAccessItem,
	AdminUserAccessReplaceRequest,
} from "../../src/api/adminUserAccess";
import type { AdminUserNodeQuotaStatusResponse } from "../../src/api/adminUserNodeQuotaStatus";
import type { AdminUserNodeQuota } from "../../src/api/adminUserNodeQuotas";
import type { AdminUserNodeWeightItem } from "../../src/api/adminUserNodeWeights";
import type { AdminUserQuotaSummariesResponse } from "../../src/api/adminUserQuotaSummaries";
import type {
	AdminUser,
	AdminUserCreateRequest,
	AdminUserPatchRequest,
	AdminUserTokenResponse,
} from "../../src/api/adminUsers";
import type { ClusterInfoResponse } from "../../src/api/clusterInfo";
import type { GrantCredentials } from "../../src/api/grantCredentials";
import type { HealthResponse } from "../../src/api/health";
import type { NodeQuotaReset, UserQuotaReset } from "../../src/api/quotaReset";
import type { VersionCheckResponse } from "../../src/api/versionCheck";

export type StorybookApiMockConfig = {
	adminToken?: string | null;
	data?: Partial<MockStateSeed>;
	failAdminConfig?: boolean;
	failVersionCheck?: boolean;
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
	realityDomains: AdminRealityDomain[];
	users: AdminUser[];
	userAccessByUser: Record<string, AdminUserAccessItem[]>;
	nodeQuotas: AdminUserNodeQuota[];
	userNodeWeights: Record<string, AdminUserNodeWeightItem[]>;
	userGlobalWeights: Record<string, number>;
	nodeWeightPolicies: Record<string, AdminQuotaPolicyNodePolicy>;
	quotaSummaries?: AdminUserQuotaSummariesResponse;
	alerts: AlertsResponse;
	subscriptions: Record<string, string>;
};

type MockState = Omit<MockStateSeed, "endpoints"> & {
	endpoints: MockEndpointRecord[];
	failAdminConfig: boolean;
	failVersionCheck: boolean;
	counters: {
		endpoint: number;
		grant: number;
		joinToken: number;
		realityDomain: number;
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
const DEFAULT_GLOBAL_WEIGHT = 100;

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

function sseResponse(
	events: Array<{ event: string; data: unknown }>,
): Response {
	const body = events
		.map((item) => {
			return `event: ${item.event}\ndata: ${JSON.stringify(item.data)}\n\n`;
		})
		.join("");
	return new Response(body, {
		status: 200,
		headers: {
			"Content-Type": "text/event-stream",
			"Cache-Control": "no-cache",
		},
	});
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

function deriveGlobalServerNames(
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

function buildRuntimeSlots(total = 7 * 24 * 2): NodeRuntimeHistorySlot[] {
	const now = new Date();
	const base = new Date(now);
	base.setSeconds(0, 0);
	base.setMinutes(base.getMinutes() < 30 ? 0 : 30);

	const slots: NodeRuntimeHistorySlot[] = [];
	for (let i = total - 1; i >= 0; i -= 1) {
		const at = new Date(base.getTime() - i * 30 * 60 * 1000);
		let status: NodeRuntimeHistorySlot["status"] = "up";
		if (i % 37 === 0) status = "degraded";
		if (i % 121 === 0) status = "down";
		if (i % 79 === 0) status = "unknown";
		slots.push({
			slot_start: at.toISOString(),
			status,
		});
	}
	return slots;
}

function buildRuntimeComponents(node: AdminNode): NodeRuntimeComponent[] {
	const downNode = node.node_id.endsWith("2");
	return [
		{
			component: "xp",
			status: "up",
			last_ok_at: new Date().toISOString(),
			last_fail_at: null,
			down_since: null,
			consecutive_failures: 0,
			recoveries_observed: 0,
			restart_attempts: 0,
			last_restart_at: null,
			last_restart_fail_at: null,
		},
		{
			component: "xray",
			status: downNode ? "down" : "up",
			last_ok_at: new Date(Date.now() - 60_000).toISOString(),
			last_fail_at: downNode
				? new Date(Date.now() - 30_000).toISOString()
				: null,
			down_since: downNode ? new Date(Date.now() - 30_000).toISOString() : null,
			consecutive_failures: downNode ? 2 : 0,
			recoveries_observed: 1,
			restart_attempts: downNode ? 1 : 0,
			last_restart_at: downNode
				? new Date(Date.now() - 20_000).toISOString()
				: null,
			last_restart_fail_at: null,
		},
		{
			component: "cloudflared",
			status: downNode ? "down" : "disabled",
			last_ok_at: downNode ? new Date(Date.now() - 90_000).toISOString() : null,
			last_fail_at: downNode
				? new Date(Date.now() - 10_000).toISOString()
				: null,
			down_since: downNode ? new Date(Date.now() - 10_000).toISOString() : null,
			consecutive_failures: downNode ? 3 : 0,
			recoveries_observed: 0,
			restart_attempts: downNode ? 1 : 0,
			last_restart_at: downNode
				? new Date(Date.now() - 10_000).toISOString()
				: null,
			last_restart_fail_at: downNode
				? new Date(Date.now() - 10_000).toISOString()
				: null,
		},
	];
}

function buildRuntimeEvents(node: AdminNode): NodeRuntimeEvent[] {
	return [
		{
			event_id: `evt-${node.node_id}-1`,
			occurred_at: new Date(Date.now() - 20_000).toISOString(),
			component: "xray",
			kind: "status_changed",
			message: "xray status changed: up -> down",
			from_status: "up",
			to_status: "down",
		},
		{
			event_id: `evt-${node.node_id}-2`,
			occurred_at: new Date(Date.now() - 10_000).toISOString(),
			component: "cloudflared",
			kind: "restart_failed",
			message: "cloudflared restart request failed",
			from_status: null,
			to_status: "down",
		},
	];
}

function buildNodeRuntimeListItem(node: AdminNode): AdminNodeRuntimeListItem {
	const components = buildRuntimeComponents(node);
	const slots = buildRuntimeSlots();
	const summaryStatus: AdminNodeRuntimeListItem["summary"]["status"] =
		components.some((component) => component.status === "down")
			? "degraded"
			: "up";
	return {
		node_id: node.node_id,
		node_name: node.node_name,
		api_base_url: node.api_base_url,
		access_host: node.access_host,
		summary: {
			status: summaryStatus,
			updated_at: new Date().toISOString(),
		},
		components,
		recent_slots: slots,
	};
}

function buildNodeRuntimeDetail(
	node: AdminNode,
): AdminNodeRuntimeDetailResponse {
	const item = buildNodeRuntimeListItem(node);
	return {
		node: node,
		summary: item.summary,
		components: item.components,
		recent_slots: item.recent_slots,
		events: buildRuntimeEvents(node),
	};
}

function refreshGlobalEndpointReality(state: MockState): void {
	for (const endpoint of state.endpoints) {
		if (endpoint.kind !== "vless_reality_vision_tcp") continue;
		const meta = endpoint.meta as Record<string, unknown>;
		const reality = meta.reality as
			| undefined
			| null
			| {
					dest?: string;
					server_names?: string[];
					server_names_source?: string;
					fingerprint?: string;
			  };
		if (!reality || typeof reality !== "object") continue;
		if (reality.server_names_source !== "global") continue;

		const derived = deriveGlobalServerNames(
			state.realityDomains,
			endpoint.node_id,
		);
		if (derived.length === 0) continue;

		meta.reality = {
			...reality,
			dest: `${derived[0]}:443`,
			server_names: derived,
			server_names_source: "global",
		};
	}
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
			quota_limit_bytes: 0,
			quota_reset: defaultNodeQuotaReset(1),
		},
		{
			node_id: "node-2",
			node_name: "osaka-1",
			api_base_url: "https://osaka-1.example.com",
			access_host: "osaka-1.example.com",
			quota_limit_bytes: 0,
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
					server_names_source: "manual",
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

	const realityDomains: AdminRealityDomain[] = [
		{
			domain_id: "seed_public_sn_files_1drv_com",
			server_name: "public.sn.files.1drv.com",
			disabled_node_ids: [],
		},
		{
			domain_id: "seed_public_bn_files_1drv_com",
			server_name: "public.bn.files.1drv.com",
			disabled_node_ids: [],
		},
		{
			domain_id: "seed_oneclient_sfx_ms",
			server_name: "oneclient.sfx.ms",
			disabled_node_ids: ["node-2"],
		},
	];

	// Keep IDs close to prod behavior: user_id is a ULID, token is `sub_<ulid>`.
	const userId1 = "01HF7YAT00T6RTJH6T9Z8ZPMDV";
	const userId2 = "01HF7YAT01YVKWQ847J5T9EY84";
	const subToken1 = `sub_${userId1}`;
	const subToken2 = `sub_${userId2}`;

	const users: AdminUser[] = [
		{
			user_id: userId1,
			display_name: "Alice",
			subscription_token: subToken1,
			priority_tier: "p3",
			quota_reset: defaultUserQuotaReset(1),
		},
		{
			user_id: userId2,
			display_name: "Bob",
			subscription_token: subToken2,
			priority_tier: "p3",
			quota_reset: defaultUserQuotaReset(15),
		},
	];

	const userNodeWeights: Record<string, AdminUserNodeWeightItem[]> = {
		[userId1]: [{ node_id: "node-1", weight: 120 }],
		[userId2]: [],
	};
	const userGlobalWeights: Record<string, number> = {
		[userId1]: 120,
		[userId2]: 80,
	};
	const nodeWeightPolicies: Record<string, AdminQuotaPolicyNodePolicy> = {
		"node-1": { node_id: "node-1", inherit_global: true },
		"node-2": { node_id: "node-2", inherit_global: true },
	};

	const userAccessByUser: Record<string, AdminUserAccessItem[]> = {
		[userId1]: [
			{
				membership: {
					user_id: userId1,
					node_id: "node-1",
					endpoint_id: "endpoint-1",
				},
				grant: {
					grant_id: "grant-mock-1",
					enabled: true,
					quota_limit_bytes: 10_000_000,
					note: "Priority",
					credentials: createGrantCredentials(endpoints[0], 1),
				},
			},
		],
		[userId2]: [
			{
				membership: {
					user_id: userId2,
					node_id: "node-2",
					endpoint_id: "endpoint-2",
				},
				grant: {
					grant_id: "grant-mock-2",
					enabled: true,
					quota_limit_bytes: 5_000_000,
					note: null,
					credentials: createGrantCredentials(endpoints[1], 2),
				},
			},
		],
	};

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
		[subToken1]: `# raw subscription for ${subToken1}\nnode-1`,
		[subToken2]: `# raw subscription for ${subToken2}\nnode-2`,
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
		realityDomains,
		users,
		userAccessByUser,
		nodeQuotas: [],
		userNodeWeights,
		userGlobalWeights,
		nodeWeightPolicies,
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
		realityDomains: overrides?.realityDomains ?? base.realityDomains,
		users: overrides?.users ?? base.users,
		userAccessByUser: overrides?.userAccessByUser ?? base.userAccessByUser,
		nodeQuotas: overrides?.nodeQuotas ?? base.nodeQuotas,
		userNodeWeights: overrides?.userNodeWeights ?? base.userNodeWeights,
		userGlobalWeights: overrides?.userGlobalWeights ?? base.userGlobalWeights,
		nodeWeightPolicies:
			overrides?.nodeWeightPolicies ?? base.nodeWeightPolicies,
		quotaSummaries: overrides?.quotaSummaries ?? base.quotaSummaries,
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
		realityDomain: 1,
		shortId: 1,
		subscription: 1,
		user: 1,
	};

	const endpoints = merged.endpoints.map((endpoint) =>
		ensureEndpointRecord(endpoint, counters),
	);

	const state: MockState = {
		...clone(merged),
		endpoints,
		failAdminConfig: config?.failAdminConfig ?? false,
		failVersionCheck: config?.failVersionCheck ?? false,
		counters,
	};

	refreshGlobalEndpointReality(state);
	return state;
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

	if (path === "/api/admin/nodes/runtime" && method === "GET") {
		const items = state.nodes.map((node) => buildNodeRuntimeListItem(node));
		return jsonResponse({
			partial: false,
			unreachable_nodes: [],
			items: clone(items),
		});
	}

	const nodeRuntimeMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/runtime$/,
	);
	if (nodeRuntimeMatch && method === "GET") {
		const nodeId = decodeURIComponent(nodeRuntimeMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		if (!node) {
			return errorResponse(404, "not_found", "node not found");
		}
		return jsonResponse(clone(buildNodeRuntimeDetail(node)));
	}

	const nodeRuntimeEventsMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/runtime\/events$/,
	);
	if (nodeRuntimeEventsMatch && method === "GET") {
		const nodeId = decodeURIComponent(nodeRuntimeEventsMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		if (!node) {
			return errorResponse(404, "not_found", "node not found");
		}
		const detail = buildNodeRuntimeDetail(node);
		return sseResponse([
			{
				event: "hello",
				data: {
					node_id: node.node_id,
					connected_at: new Date().toISOString(),
				},
			},
			{
				event: "snapshot",
				data: {
					node_id: node.node_id,
					summary: detail.summary,
					components: detail.components,
					recent_slots: detail.recent_slots,
					events: detail.events,
				},
			},
		]);
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
		// Deprecated: static per-user node quotas are no longer editable.
		return errorResponse(
			410,
			"gone",
			"user node quotas are no longer editable; configure node quota_limit_bytes + user node weights instead",
		);
	}

	const userNodeWeightsMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-weights$/,
	);
	if (userNodeWeightsMatch && method === "GET") {
		const userId = decodeURIComponent(userNodeWeightsMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const items = state.userNodeWeights[userId] ?? [];
		return jsonResponse({ items: clone(items) });
	}

	const userNodeWeightPutMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-weights\/([^/]+)$/,
	);
	if (userNodeWeightPutMatch && method === "PUT") {
		const userId = decodeURIComponent(userNodeWeightPutMatch[1]);
		const nodeId = decodeURIComponent(userNodeWeightPutMatch[2]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const nodeExists = state.nodes.some((n) => n.node_id === nodeId);
		if (!nodeExists) {
			return errorResponse(404, "not_found", "node not found");
		}

		const payload = await readJson<{ weight?: number }>(req);
		if (!payload || typeof payload.weight !== "number") {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		if (!Number.isFinite(payload.weight) || !Number.isInteger(payload.weight)) {
			return errorResponse(400, "invalid_request", "weight must be an integer");
		}
		if (payload.weight < 0 || payload.weight > 65535) {
			return errorResponse(
				400,
				"invalid_request",
				"weight must be between 0 and 65535",
			);
		}

		const items = state.userNodeWeights[userId] ?? [];
		const next: AdminUserNodeWeightItem = {
			node_id: nodeId,
			weight: payload.weight,
		};
		state.userNodeWeights[userId] = [
			...items.filter((i) => i.node_id !== nodeId),
			next,
		];

		return jsonResponse(clone(next));
	}

	const quotaPolicyNodeWeightRowsMatch = path.match(
		/^\/api\/admin\/quota-policy\/nodes\/([^/]+)\/weight-rows$/,
	);
	if (
		path === "/api/admin/quota-policy/global-weight-rows" &&
		method === "GET"
	) {
		const items: AdminQuotaPolicyGlobalWeightRow[] = state.users.map((user) => {
			const storedWeight = state.userGlobalWeights[user.user_id];
			return {
				user_id: user.user_id,
				display_name: user.display_name,
				priority_tier: user.priority_tier,
				stored_weight: storedWeight,
				editor_weight: storedWeight ?? DEFAULT_GLOBAL_WEIGHT,
				source: storedWeight === undefined ? "implicit_default" : "explicit",
			};
		});
		items.sort(
			(a, b) =>
				b.editor_weight - a.editor_weight || a.user_id.localeCompare(b.user_id),
		);
		return jsonResponse({ items: clone(items) });
	}

	const quotaPolicyGlobalWeightPutMatch = path.match(
		/^\/api\/admin\/quota-policy\/global-weight-rows\/([^/]+)$/,
	);
	if (quotaPolicyGlobalWeightPutMatch && method === "PUT") {
		const userId = decodeURIComponent(quotaPolicyGlobalWeightPutMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}

		const payload = await readJson<{ weight?: number }>(req);
		if (!payload || typeof payload.weight !== "number") {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		if (!Number.isFinite(payload.weight) || !Number.isInteger(payload.weight)) {
			return errorResponse(400, "invalid_request", "weight must be an integer");
		}
		if (payload.weight < 0 || payload.weight > 65535) {
			return errorResponse(
				400,
				"invalid_request",
				"weight must be between 0 and 65535",
			);
		}

		state.userGlobalWeights[userId] = payload.weight;
		return jsonResponse({ user_id: userId, weight: payload.weight });
	}

	const quotaPolicyNodePolicyMatch = path.match(
		/^\/api\/admin\/quota-policy\/nodes\/([^/]+)\/policy$/,
	);
	if (quotaPolicyNodePolicyMatch) {
		const nodeId = decodeURIComponent(quotaPolicyNodePolicyMatch[1]);
		const nodeExists = state.nodes.some((node) => node.node_id === nodeId);
		if (!nodeExists) {
			return errorResponse(404, "not_found", "node not found");
		}
		if (method === "GET") {
			return jsonResponse(
				clone(
					state.nodeWeightPolicies[nodeId] ?? {
						node_id: nodeId,
						inherit_global: true,
					},
				),
			);
		}
		if (method === "PUT") {
			const payload = await readJson<{ inherit_global?: boolean }>(req);
			if (!payload || typeof payload.inherit_global !== "boolean") {
				return errorResponse(
					400,
					"invalid_request",
					"inherit_global must be a boolean",
				);
			}
			const nextPolicy: AdminQuotaPolicyNodePolicy = {
				node_id: nodeId,
				inherit_global: payload.inherit_global,
			};
			state.nodeWeightPolicies[nodeId] = nextPolicy;
			return jsonResponse(clone(nextPolicy));
		}
	}

	if (quotaPolicyNodeWeightRowsMatch && method === "GET") {
		const nodeId = decodeURIComponent(quotaPolicyNodeWeightRowsMatch[1]);
		const nodeExists = state.nodes.some((node) => node.node_id === nodeId);
		if (!nodeExists) {
			return errorResponse(404, "not_found", "node not found");
		}

		const endpointNodeById = new Map(
			state.endpoints.map((endpoint) => [
				endpoint.endpoint_id,
				endpoint.node_id,
			]),
		);
		const endpointIdsByUser = new Map<string, Set<string>>();
		for (const items of Object.values(state.userAccessByUser)) {
			for (const item of items) {
				if (!item.grant.enabled) continue;
				const endpointNodeId = endpointNodeById.get(
					item.membership.endpoint_id,
				);
				if (!endpointNodeId || endpointNodeId !== nodeId) continue;
				const userId = item.membership.user_id;
				if (!endpointIdsByUser.has(userId)) {
					endpointIdsByUser.set(userId, new Set<string>());
				}
				endpointIdsByUser.get(userId)?.add(item.membership.endpoint_id);
			}
		}

		const items: AdminQuotaPolicyNodeWeightRow[] = [];
		for (const [userId, endpointIdsSet] of endpointIdsByUser.entries()) {
			const user = state.users.find(
				(candidate) => candidate.user_id === userId,
			);
			if (!user) {
				continue;
			}
			const storedWeight = (state.userNodeWeights[userId] ?? []).find(
				(entry) => entry.node_id === nodeId,
			)?.weight;
			items.push({
				user_id: user.user_id,
				display_name: user.display_name,
				priority_tier: user.priority_tier,
				endpoint_ids: [...endpointIdsSet].sort(),
				stored_weight: storedWeight,
				editor_weight: storedWeight ?? 0,
				source: storedWeight === undefined ? "implicit_zero" : "explicit",
			});
		}
		items.sort(
			(a, b) =>
				b.editor_weight - a.editor_weight || a.user_id.localeCompare(b.user_id),
		);
		return jsonResponse({ items: clone(items) });
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
				quota_limit_bytes?: number;
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
				quota_limit_bytes: payload.quota_limit_bytes ?? node.quota_limit_bytes,
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

	if (path === "/api/admin/reality-domains" && method === "GET") {
		return jsonResponse({ items: clone(state.realityDomains) });
	}

	if (path === "/api/admin/reality-domains" && method === "POST") {
		const payload = await readJson<{
			server_name?: string;
			disabled_node_ids?: string[];
		}>(req);
		if (!payload || typeof payload.server_name !== "string") {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		const serverName = payload.server_name.trim();
		if (!serverName) {
			return errorResponse(
				400,
				"invalid_request",
				"server_name must be non-empty",
			);
		}
		const domain: AdminRealityDomain = {
			domain_id: `domain-mock-${state.counters.realityDomain++}`,
			server_name: serverName,
			disabled_node_ids: payload.disabled_node_ids ?? [],
		};
		state.realityDomains = [...state.realityDomains, domain];
		refreshGlobalEndpointReality(state);
		return jsonResponse(clone(domain));
	}

	if (path === "/api/admin/reality-domains/reorder" && method === "POST") {
		const payload = await readJson<{ domain_ids?: string[] }>(req);
		const ids = payload?.domain_ids;
		if (
			!payload ||
			!Array.isArray(ids) ||
			!ids.every((id) => typeof id === "string")
		) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}

		const byId = new Map(state.realityDomains.map((d) => [d.domain_id, d]));
		const next: AdminRealityDomain[] = [];
		for (const id of ids) {
			const domain = byId.get(id);
			if (!domain) {
				return errorResponse(
					400,
					"invalid_request",
					`unknown domain_id: ${id}`,
				);
			}
			next.push(domain);
		}
		state.realityDomains = next;
		refreshGlobalEndpointReality(state);
		return new Response(null, { status: 204 });
	}

	const realityDomainMatch = path.match(
		/^\/api\/admin\/reality-domains\/([^/]+)$/,
	);
	if (realityDomainMatch) {
		const domainId = decodeURIComponent(realityDomainMatch[1]);
		const existing = state.realityDomains.find((d) => d.domain_id === domainId);
		if (!existing) {
			return errorResponse(404, "not_found", "reality domain not found");
		}
		if (method === "PATCH") {
			const payload = await readJson<{
				server_name?: string;
				disabled_node_ids?: string[];
			}>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const updated: AdminRealityDomain = {
				...existing,
				server_name:
					typeof payload.server_name === "string"
						? payload.server_name.trim()
						: existing.server_name,
				disabled_node_ids: Array.isArray(payload.disabled_node_ids)
					? payload.disabled_node_ids
					: existing.disabled_node_ids,
			};
			state.realityDomains = state.realityDomains.map((d) =>
				d.domain_id === domainId ? updated : d,
			);
			refreshGlobalEndpointReality(state);
			return jsonResponse(clone(updated));
		}
		if (method === "DELETE") {
			state.realityDomains = state.realityDomains.filter(
				(d) => d.domain_id !== domainId,
			);
			refreshGlobalEndpointReality(state);
			return new Response(null, { status: 204 });
		}
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
			const source = payload.reality.server_names_source ?? "manual";
			const derived =
				source === "global"
					? deriveGlobalServerNames(state.realityDomains, payload.node_id)
					: payload.reality.server_names;
			if (!derived || derived.length === 0) {
				return errorResponse(
					400,
					"invalid_request",
					"server_names must be non-empty",
				);
			}
			const normalizedReality = {
				...payload.reality,
				dest: `${derived[0]}:443`,
				server_names: derived,
				server_names_source: source,
			};
			meta = {
				reality: normalizedReality,
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

	if (path === "/api/admin/users/quota-summaries" && method === "GET") {
		if (state.quotaSummaries) {
			return jsonResponse(clone(state.quotaSummaries));
		}

		const totals = new Map<
			string,
			{ quota_limit_bytes: number; used_bytes: number; remaining_bytes: number }
		>();
		for (const q of state.nodeQuotas) {
			const prev = totals.get(q.user_id);

			// Keep semantics consistent with the backend:
			// `quota_limit_bytes === 0` means "unlimited".
			// Important: the first seen node quota must not be treated as "unlimited"
			// just because our accumulator starts at 0.
			const nextLimit = !prev
				? q.quota_limit_bytes
				: prev.quota_limit_bytes === 0 || q.quota_limit_bytes === 0
					? 0
					: prev.quota_limit_bytes + q.quota_limit_bytes;

			totals.set(q.user_id, {
				quota_limit_bytes: nextLimit,
				used_bytes: 0,
				remaining_bytes: nextLimit === 0 ? 0 : nextLimit,
			});
		}
		// Only include users that have any quota data (real API omits users without quotas).
		const items = state.users.flatMap((u) => {
			const t = totals.get(u.user_id);
			if (!t) return [];
			return [
				{
					user_id: u.user_id,
					quota_limit_kind:
						t.quota_limit_bytes === 0
							? ("unlimited" as const)
							: ("fixed" as const),
					quota_limit_bytes: t.quota_limit_bytes,
					used_bytes: t.used_bytes,
					remaining_bytes: t.remaining_bytes,
				},
			];
		});

		const response: AdminUserQuotaSummariesResponse = {
			partial: false,
			unreachable_nodes: [],
			items,
		};
		return jsonResponse(response);
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
			priority_tier: "p2",
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
				priority_tier: payload.priority_tier ?? user.priority_tier,
				quota_reset: payload.quota_reset ?? user.quota_reset,
			};
			state.users = state.users.map((item) =>
				item.user_id === userId ? updated : item,
			);
			return jsonResponse(clone(updated));
		}
		if (method === "DELETE") {
			state.users = state.users.filter((item) => item.user_id !== userId);
			delete state.userAccessByUser[userId];
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

	const userNodeQuotaStatusMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-quotas\/status$/,
	);
	if (userNodeQuotaStatusMatch && method === "GET") {
		const userId = decodeURIComponent(userNodeQuotaStatusMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const cycleEnd = new Date(
			Date.now() + 10 * 24 * 60 * 60 * 1000,
		).toISOString();
		const items = state.nodeQuotas
			.filter((q) => q.user_id === userId)
			.map((q) => ({
				user_id: q.user_id,
				node_id: q.node_id,
				quota_limit_bytes: q.quota_limit_bytes,
				used_bytes: 0,
				remaining_bytes: q.quota_limit_bytes,
				cycle_end_at: cycleEnd,
				quota_reset_source: q.quota_reset_source,
			}));

		const response: AdminUserNodeQuotaStatusResponse = {
			partial: false,
			unreachable_nodes: [],
			items,
		};
		return jsonResponse(response);
	}

	const userAccessMatch = path.match(/^\/api\/admin\/users\/([^/]+)\/access$/);
	if (userAccessMatch) {
		const userId = decodeURIComponent(userAccessMatch[1]);
		const userExists = state.users.some((user) => user.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}

		if (method === "GET") {
			const items = clone(state.userAccessByUser[userId] ?? []);
			items.sort(
				(a, b) =>
					a.membership.node_id.localeCompare(b.membership.node_id) ||
					a.membership.endpoint_id.localeCompare(b.membership.endpoint_id) ||
					a.grant.grant_id.localeCompare(b.grant.grant_id),
			);
			return jsonResponse({ items });
		}

		if (method === "PUT") {
			const payload = await readJson<AdminUserAccessReplaceRequest>(req);
			if (!payload || !Array.isArray(payload.items)) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}

			const endpointsById = new Map(
				state.endpoints.map(({ active_short_id, short_ids, ...rest }) => [
					rest.endpoint_id,
					rest,
				]),
			);
			const existingByEndpoint = new Map(
				(state.userAccessByUser[userId] ?? []).map((item) => [
					item.membership.endpoint_id,
					item,
				]),
			);
			const seenEndpointIds = new Set<string>();
			const nextItems: AdminUserAccessItem[] = [];

			for (const item of payload.items) {
				const endpointId = String(item.endpoint_id ?? "").trim();
				if (!endpointId) {
					return errorResponse(
						400,
						"invalid_request",
						"endpoint_id is required",
					);
				}
				if (seenEndpointIds.has(endpointId)) {
					return errorResponse(
						400,
						"invalid_request",
						`duplicate endpoint_id in access payload: ${endpointId}`,
					);
				}
				seenEndpointIds.add(endpointId);

				const endpoint = endpointsById.get(endpointId);
				if (!endpoint) {
					return errorResponse(400, "invalid_request", "endpoint not found");
				}
				const previous = existingByEndpoint.get(endpointId);
				const grantCounter = state.counters.grant++;

				nextItems.push({
					membership: {
						user_id: userId,
						node_id: endpoint.node_id,
						endpoint_id: endpointId,
					},
					grant: {
						grant_id: previous?.grant.grant_id ?? `grant-mock-${grantCounter}`,
						enabled: true,
						quota_limit_bytes: previous?.grant.quota_limit_bytes ?? 0,
						note: item.note ?? null,
						credentials:
							previous?.grant.credentials ??
							createGrantCredentials(endpoint, grantCounter),
					},
				});
			}

			nextItems.sort(
				(a, b) =>
					a.membership.node_id.localeCompare(b.membership.node_id) ||
					a.membership.endpoint_id.localeCompare(b.membership.endpoint_id) ||
					a.grant.grant_id.localeCompare(b.grant.grant_id),
			);
			state.userAccessByUser[userId] = nextItems;
			return jsonResponse({ items: clone(nextItems) });
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
