import type { Page, Route } from "@playwright/test";
import yaml from "js-yaml";

const { dump, load } = yaml;

type QuotaResetSource = "user" | "node";

type UserQuotaReset =
	| { policy: "unlimited"; tz_offset_minutes: number }
	| {
			policy: "monthly";
			day_of_month: number;
			tz_offset_minutes: number;
	  };

type NodeQuotaReset =
	| { policy: "unlimited"; tz_offset_minutes?: number | null }
	| {
			policy: "monthly";
			day_of_month: number;
			tz_offset_minutes?: number | null;
	  };

type AdminUser = {
	user_id: string;
	display_name: string;
	subscription_token: string;
	credential_epoch: number;
	priority_tier: "p1" | "p2" | "p3";
	quota_reset: UserQuotaReset;
};

type AdminNode = {
	node_id: string;
	node_name: string;
	api_base_url: string;
	access_host: string;
	quota_limit_bytes: number;
	quota_reset: NodeQuotaReset;
};

type AdminEndpoint = {
	endpoint_id: string;
	node_id: string;
	tag: string;
	kind: "vless_reality_vision_tcp" | "ss2022_2022_blake3_aes_128_gcm";
	port: number;
	meta: Record<string, unknown>;
};

type AdminUserNodeQuota = {
	user_id: string;
	node_id: string;
	quota_limit_bytes: number;
	quota_reset_source: QuotaResetSource;
};

type AdminUserNodeWeightItem = {
	node_id: string;
	weight: number;
};

type AdminUserAccessItem = {
	user_id: string;
	endpoint_id: string;
	node_id: string;
};

type ClusterInfo = {
	cluster_id: string;
	node_id: string;
	role: string;
	leader_api_base_url: string;
	term: number;
};

type AlertsResponse = {
	partial: boolean;
	unreachable_nodes: string[];
	items: Array<{
		type: string;
		membership_key: string;
		user_id: string;
		endpoint_id: string;
		owner_node_id: string;
		quota_banned: boolean;
		quota_banned_at: string | null;
		message: string;
		action_hint: string;
	}>;
};

type MockMihomoProfile = {
	mixin_yaml?: string;
	template_yaml?: string;
	extra_proxies_yaml: string;
	extra_proxy_providers_yaml: string;
};

type MockApiOptions = {
	users?: AdminUser[];
	nodes?: AdminNode[];
	endpoints?: AdminEndpoint[];
	nodeQuotas?: AdminUserNodeQuota[];
	userNodeWeights?: Record<string, AdminUserNodeWeightItem[]>;
	userAccessByUserId?: Record<string, AdminUserAccessItem[]>;
	clusterInfo?: ClusterInfo;
	alerts?: AlertsResponse;
	healthStatus?: "ok" | "error";
	subscriptionContentRaw?: string;
	subscriptionContentClash?: string;
	userMihomoProfiles?: Record<string, MockMihomoProfile>;
};

type MockState = {
	users: AdminUser[];
	nodes: AdminNode[];
	endpoints: AdminEndpoint[];
	nodeQuotas: AdminUserNodeQuota[];
	userNodeWeights: Record<string, AdminUserNodeWeightItem[]>;
	userAccessByUserId: Record<string, AdminUserAccessItem[]>;
	clusterInfo: ClusterInfo;
	alerts: AlertsResponse;
	healthStatus: "ok" | "error";
	subscriptionContentRaw: string;
	subscriptionContentClash: string;
	userMihomoProfiles: Record<string, MockMihomoProfile>;
};

const defaultNodes: AdminNode[] = [
	{
		node_id: "node-1",
		node_name: "alpha",
		api_base_url: "http://127.0.0.1:62416",
		access_host: "alpha.example.com",
		quota_limit_bytes: 0,
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		},
	},
];

const defaultEndpoints: AdminEndpoint[] = [
	{
		endpoint_id: "endpoint-1",
		node_id: "node-1",
		tag: "edge-1",
		kind: "vless_reality_vision_tcp",
		port: 443,
		meta: {},
	},
];

const defaultUsers: AdminUser[] = [
	{
		user_id: "user-1",
		display_name: "Demo user",
		subscription_token: "sub-user-1",
		credential_epoch: 0,
		priority_tier: "p3",
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: 480,
		},
	},
];

const defaultUserAccessByUserId: Record<string, AdminUserAccessItem[]> = {
	"user-1": [
		{ user_id: "user-1", endpoint_id: "endpoint-1", node_id: "node-1" },
	],
};

const defaultClusterInfo: ClusterInfo = {
	cluster_id: "cluster-1",
	node_id: "node-1",
	role: "leader",
	leader_api_base_url: "http://127.0.0.1:62416",
	term: 1,
};

const defaultAlerts: AlertsResponse = {
	partial: false,
	unreachable_nodes: [],
	items: [],
};

const defaultSubscriptionClash = `proxies:
  - name: demo
    type: vless
    servername: example.com
    reality-opts:
      public-key: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
      short-id: 0123456789abcdef
`;

function jsonResponse(route: Route, payload: unknown, status = 200): void {
	void route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function textResponse(route: Route, payload: string, status = 200): void {
	void route.fulfill({
		status,
		contentType: "text/plain",
		body: payload,
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

function parseJsonBody(request: { postData(): string | null }): Record<
	string,
	unknown
> {
	const raw = request.postData();
	if (!raw) return {};
	try {
		const parsed = JSON.parse(raw) as Record<string, unknown>;
		return parsed ?? {};
	} catch {
		return {};
	}
}

type CanonicalMockMihomoProfile = {
	mixin_yaml: string;
	extra_proxies_yaml: string;
	extra_proxy_providers_yaml: string;
};

type MockMihomoProfileNormalizationResult =
	| { ok: true; profile: CanonicalMockMihomoProfile }
	| { ok: false; message: string };

function formatYamlError(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function isYamlMapping(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function canonicalizeMockMihomoProfile(
	profile: Partial<MockMihomoProfile> | undefined,
): CanonicalMockMihomoProfile {
	return {
		mixin_yaml:
			typeof profile?.mixin_yaml === "string"
				? profile.mixin_yaml
				: typeof profile?.template_yaml === "string"
					? profile.template_yaml
					: "",
		extra_proxies_yaml:
			typeof profile?.extra_proxies_yaml === "string"
				? profile.extra_proxies_yaml
				: "",
		extra_proxy_providers_yaml:
			typeof profile?.extra_proxy_providers_yaml === "string"
				? profile.extra_proxy_providers_yaml
				: "",
	};
}

function ensureYamlSequenceOrEmpty(
	raw: string,
	fieldName: string,
): MockMihomoProfileNormalizationResult | null {
	if (raw.trim() === "") {
		return null;
	}
	let value: unknown;
	try {
		value = load(raw);
	} catch (error) {
		return {
			ok: false,
			message: `${fieldName} must be valid yaml: ${formatYamlError(error)}`,
		};
	}
	if (!Array.isArray(value)) {
		return {
			ok: false,
			message: `${fieldName} must be a yaml sequence or empty string`,
		};
	}
	return null;
}

function ensureYamlMappingOrEmpty(
	raw: string,
	fieldName: string,
): MockMihomoProfileNormalizationResult | null {
	if (raw.trim() === "") {
		return null;
	}
	let value: unknown;
	try {
		value = load(raw);
	} catch (error) {
		return {
			ok: false,
			message: `${fieldName} must be valid yaml: ${formatYamlError(error)}`,
		};
	}
	if (!isYamlMapping(value)) {
		return {
			ok: false,
			message: `${fieldName} must be a yaml mapping or empty string`,
		};
	}
	return null;
}

export function normalizeMockMihomoProfilePayload(
	payload: Record<string, unknown>,
): MockMihomoProfileNormalizationResult {
	const canonical = canonicalizeMockMihomoProfile(payload);
	if (canonical.mixin_yaml.trim() === "") {
		return { ok: false, message: "mixin_yaml is required" };
	}

	let mixinRoot: unknown;
	try {
		mixinRoot = load(canonical.mixin_yaml);
	} catch (error) {
		return {
			ok: false,
			message: `mixin_yaml must be valid yaml: ${formatYamlError(error)}`,
		};
	}
	if (!isYamlMapping(mixinRoot)) {
		return { ok: false, message: "mixin_yaml must be a yaml mapping" };
	}

	let mixinMap: Record<string, unknown> = { ...mixinRoot };
	let mixin_yaml = canonical.mixin_yaml;
	let extra_proxies_yaml = canonical.extra_proxies_yaml;
	let extra_proxy_providers_yaml = canonical.extra_proxy_providers_yaml;
	let extracted = false;

	if (Object.prototype.hasOwnProperty.call(mixinMap, "proxies")) {
		const value = mixinMap.proxies;
		if (!Array.isArray(value)) {
			return {
				ok: false,
				message: "mixin_yaml.proxies must be a yaml sequence",
			};
		}
		if (extra_proxies_yaml.trim() !== "") {
			return {
				ok: false,
				message:
					"mixin_yaml.proxies cannot be combined with extra_proxies_yaml",
			};
		}
		extra_proxies_yaml = dump(value);
		const { proxies: _removedProxies, ...nextMixinMap } = mixinMap;
		mixinMap = nextMixinMap;
		extracted = true;
	}

	if (Object.prototype.hasOwnProperty.call(mixinMap, "proxy-providers")) {
		const value = mixinMap["proxy-providers"];
		if (!isYamlMapping(value)) {
			return {
				ok: false,
				message: "mixin_yaml.proxy-providers must be a yaml mapping",
			};
		}
		if (extra_proxy_providers_yaml.trim() !== "") {
			return {
				ok: false,
				message:
					"mixin_yaml.proxy-providers cannot be combined with extra_proxy_providers_yaml",
			};
		}
		extra_proxy_providers_yaml = dump(value);
		const { "proxy-providers": _removedProxyProviders, ...nextMixinMap } =
			mixinMap;
		mixinMap = nextMixinMap;
		extracted = true;
	}

	if (extracted) {
		mixin_yaml = dump(mixinMap);
	}

	const extraProxiesError = ensureYamlSequenceOrEmpty(
		extra_proxies_yaml,
		"extra_proxies_yaml",
	);
	if (extraProxiesError) {
		return extraProxiesError;
	}
	const extraProxyProvidersError = ensureYamlMappingOrEmpty(
		extra_proxy_providers_yaml,
		"extra_proxy_providers_yaml",
	);
	if (extraProxyProvidersError) {
		return extraProxyProvidersError;
	}

	return {
		ok: true,
		profile: {
			mixin_yaml,
			extra_proxies_yaml,
			extra_proxy_providers_yaml,
		},
	};
}

export function normalizeMockStoredMihomoProfile(
	profile: MockMihomoProfile | undefined,
): CanonicalMockMihomoProfile {
	const canonical = canonicalizeMockMihomoProfile(profile);
	const normalized = normalizeMockMihomoProfilePayload(canonical);
	return normalized.ok ? normalized.profile : canonical;
}

export async function setupApiMocks(
	page: Page,
	options: MockApiOptions = {},
): Promise<MockState> {
	const state: MockState = {
		users: options.users ? [...options.users] : [...defaultUsers],
		nodes: options.nodes ? [...options.nodes] : [...defaultNodes],
		endpoints: options.endpoints
			? [...options.endpoints]
			: [...defaultEndpoints],
		nodeQuotas: options.nodeQuotas ? [...options.nodeQuotas] : [],
		userNodeWeights: options.userNodeWeights
			? Object.fromEntries(
					Object.entries(options.userNodeWeights).map(([userId, items]) => [
						userId,
						[...items],
					]),
				)
			: {},
		userAccessByUserId: options.userAccessByUserId
			? Object.fromEntries(
					Object.entries(options.userAccessByUserId).map(([userId, items]) => [
						userId,
						[...items],
					]),
				)
			: Object.fromEntries(
					Object.entries(defaultUserAccessByUserId).map(([userId, items]) => [
						userId,
						[...items],
					]),
				),
		clusterInfo: options.clusterInfo ?? { ...defaultClusterInfo },
		alerts: options.alerts ?? { ...defaultAlerts },
		healthStatus: options.healthStatus ?? "ok",
		subscriptionContentRaw:
			options.subscriptionContentRaw ?? "vless://example-host?encryption=none",
		subscriptionContentClash:
			options.subscriptionContentClash ?? defaultSubscriptionClash,
		userMihomoProfiles:
			options.userMihomoProfiles ??
			Object.fromEntries(
				(options.users ? options.users : defaultUsers).map((user) => [
					user.user_id,
					{
						mixin_yaml: "",
						extra_proxies_yaml: "",
						extra_proxy_providers_yaml: "",
					},
				]),
			),
	};

	let userSeq = state.users.length + 1;
	let tokenSeq = 1;

	await page.route("**/api/**", async (route) => {
		const request = route.request();
		const url = new URL(request.url());
		const path = url.pathname;
		const method = request.method();
		if (!path.startsWith("/api/")) {
			void route.continue();
			return;
		}

		if (path === "/api/health" && method === "GET") {
			jsonResponse(route, { status: state.healthStatus });
			return;
		}

		if (path === "/api/cluster/info" && method === "GET") {
			jsonResponse(route, state.clusterInfo);
			return;
		}

		if (path === "/api/admin/alerts" && method === "GET") {
			jsonResponse(route, state.alerts);
			return;
		}

		if (path === "/api/admin/nodes" && method === "GET") {
			jsonResponse(route, { items: state.nodes });
			return;
		}

		if (path === "/api/admin/nodes/runtime" && method === "GET") {
			const items = state.nodes.map((node) => ({
				node_id: node.node_id,
				node_name: node.node_name,
				api_base_url: node.api_base_url,
				access_host: node.access_host,
				summary: {
					status: "up",
					updated_at: "2026-03-01T00:00:00Z",
				},
				components: [
					{
						component: "xp",
						status: "up",
						consecutive_failures: 0,
						recoveries_observed: 1,
						restart_attempts: 0,
					},
				],
				recent_slots: [
					{
						slot_start: "2026-03-01T00:00:00Z",
						status: "up",
					},
				],
			}));
			jsonResponse(route, {
				partial: false,
				unreachable_nodes: [],
				items,
			});
			return;
		}

		const nodeGetMatch = path.match(/^\/api\/admin\/nodes\/([^/]+)$/);
		if (nodeGetMatch && method === "GET") {
			const nodeId = decodeURIComponent(nodeGetMatch[1]);
			const node = state.nodes.find((n) => n.node_id === nodeId);
			if (!node) {
				errorResponse(route, `node not found: ${nodeId}`, 404);
				return;
			}
			jsonResponse(route, node);
			return;
		}

		const nodePatchMatch = path.match(/^\/api\/admin\/nodes\/([^/]+)$/);
		if (nodePatchMatch && method === "PATCH") {
			const nodeId = decodeURIComponent(nodePatchMatch[1]);
			const node = state.nodes.find((n) => n.node_id === nodeId);
			if (!node) {
				errorResponse(route, `node not found: ${nodeId}`, 404);
				return;
			}
			const payload = parseJsonBody(request);
			if (typeof payload.quota_limit_bytes === "number") {
				node.quota_limit_bytes = payload.quota_limit_bytes;
			}
			if (payload.quota_reset) {
				node.quota_reset = payload.quota_reset as NodeQuotaReset;
			}
			jsonResponse(route, node);
			return;
		}

		if (path === "/api/admin/endpoints" && method === "GET") {
			jsonResponse(route, { items: state.endpoints });
			return;
		}

		if (path === "/api/admin/users" && method === "GET") {
			jsonResponse(route, { items: state.users });
			return;
		}

		if (path === "/api/admin/users/quota-summaries" && method === "GET") {
			jsonResponse(route, {
				partial: false,
				unreachable_nodes: [],
				items: state.users.map((u) => ({
					user_id: u.user_id,
					quota_limit_kind: "unlimited",
					quota_limit_bytes: 0,
					used_bytes: 0,
					remaining_bytes: 0,
				})),
			});
			return;
		}

		if (path === "/api/admin/users" && method === "POST") {
			const payload = parseJsonBody(request);
			const displayName =
				typeof payload.display_name === "string"
					? payload.display_name
					: `User ${userSeq}`;
			const quotaReset = payload.quota_reset as UserQuotaReset | undefined;
			const userId = `user-${userSeq++}`;
			const newUser: AdminUser = {
				user_id: userId,
				display_name: displayName,
				subscription_token: `sub-${userId}`,
				credential_epoch: 0,
				priority_tier: "p3",
				quota_reset:
					quotaReset ??
					({
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: 480,
					} satisfies UserQuotaReset),
			};
			state.users.push(newUser);
			state.userAccessByUserId[userId] = [];
			jsonResponse(route, newUser, 201);
			return;
		}

		const userAccessMatch = path.match(
			/^\/api\/admin\/users\/([^/]+)\/access$/,
		);
		if (userAccessMatch && method === "GET") {
			const userId = decodeURIComponent(userAccessMatch[1]);
			const user = state.users.find((item) => item.user_id === userId);
			if (!user) {
				errorResponse(route, `User not found: ${userId}`, 404);
				return;
			}
			jsonResponse(route, { items: state.userAccessByUserId[userId] ?? [] });
			return;
		}

		if (userAccessMatch && method === "PUT") {
			const userId = decodeURIComponent(userAccessMatch[1]);
			const user = state.users.find((item) => item.user_id === userId);
			if (!user) {
				errorResponse(route, `User not found: ${userId}`, 404);
				return;
			}

			const payload = parseJsonBody(request);
			const items = Array.isArray(payload.items) ? payload.items : null;
			if (!items) {
				errorResponse(route, "invalid access payload", 400);
				return;
			}

			const endpointById = new Map(
				state.endpoints.map((endpoint) => [endpoint.endpoint_id, endpoint]),
			);

			const desired = new Set<string>();
			for (const item of items) {
				const endpointId =
					typeof item.endpoint_id === "string" ? item.endpoint_id : "";
				if (!endpointId) {
					errorResponse(route, "invalid endpoint_id", 400);
					return;
				}
				const endpoint = endpointById.get(endpointId);
				if (!endpoint) {
					errorResponse(route, `endpoint not found: ${endpointId}`, 404);
					return;
				}
				desired.add(endpointId);
			}

			const existing = state.userAccessByUserId[userId] ?? [];
			const existingIds = new Set(existing.map((i) => i.endpoint_id));

			let created = 0;
			let deleted = 0;
			for (const id of desired) {
				if (!existingIds.has(id)) created += 1;
			}
			for (const id of existingIds) {
				if (!desired.has(id)) deleted += 1;
			}

			const nextItems: AdminUserAccessItem[] = [...desired]
				.sort()
				.map((endpointId) => ({
					user_id: userId,
					endpoint_id: endpointId,
					node_id: endpointById.get(endpointId)?.node_id ?? "",
				}));
			state.userAccessByUserId[userId] = nextItems;

			jsonResponse(route, { created, deleted, items: nextItems });
			return;
		}

		const userNodeQuotasMatch = path.match(
			/^\/api\/admin\/users\/([^/]+)\/node-quotas$/,
		);
		if (userNodeQuotasMatch && method === "GET") {
			const userId = decodeURIComponent(userNodeQuotasMatch[1]);
			jsonResponse(route, {
				items: state.nodeQuotas.filter((q) => q.user_id === userId),
			});
			return;
		}

		const userNodeQuotaPutMatch = path.match(
			/^\/api\/admin\/users\/([^/]+)\/node-quotas\/([^/]+)$/,
		);
		if (userNodeQuotaPutMatch && method === "PUT") {
			// Legacy static quotas are deprecated by the shared node quota policy.
			errorResponse(route, "deprecated endpoint: use quota policy API", 410);
			return;
		}

		if (path.startsWith("/api/admin/users/")) {
			const segments = path.split("/");
			const userId = decodeURIComponent(segments[4] ?? "");
			const isResetToken = segments[5] === "reset-token";
			const isResetCredentials = segments[5] === "reset-credentials";
			const isNodeWeights = segments[5] === "node-weights";
			const isMihomoProfile = segments[5] === "subscription-mihomo-profile";

			if (isNodeWeights && method === "GET") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				jsonResponse(route, { items: state.userNodeWeights[userId] ?? [] });
				return;
			}

			const nodeWeightPutMatch = path.match(
				/^\/api\/admin\/users\/([^/]+)\/node-weights\/([^/]+)$/,
			);
			if (nodeWeightPutMatch && method === "PUT") {
				const nodeId = decodeURIComponent(nodeWeightPutMatch[2]);
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				const node = state.nodes.find((item) => item.node_id === nodeId);
				if (!node) {
					errorResponse(route, `Node not found: ${nodeId}`, 404);
					return;
				}
				const payload = parseJsonBody(request);
				const rawWeight = payload.weight;
				if (typeof rawWeight !== "number") {
					errorResponse(route, "invalid JSON payload: missing weight", 400);
					return;
				}
				if (!Number.isFinite(rawWeight) || !Number.isInteger(rawWeight)) {
					errorResponse(route, "invalid weight: must be an integer", 400);
					return;
				}
				if (rawWeight < 0 || rawWeight > 65535) {
					errorResponse(
						route,
						"invalid weight: must be between 0 and 65535",
						400,
					);
					return;
				}

				const items = state.userNodeWeights[userId] ?? [];
				const next: AdminUserNodeWeightItem = {
					node_id: nodeId,
					weight: rawWeight,
				};
				state.userNodeWeights[userId] = [
					...items.filter((i) => i.node_id !== nodeId),
					next,
				];

				jsonResponse(route, next);
				return;
			}

			if (isResetToken && method === "POST") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				const nextToken = `reset-${tokenSeq++}-${userId}`;
				user.subscription_token = nextToken;
				jsonResponse(route, { subscription_token: nextToken });
				return;
			}

			if (isResetCredentials && method === "POST") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				user.credential_epoch += 1;
				jsonResponse(route, {
					user_id: user.user_id,
					credential_epoch: user.credential_epoch,
				});
				return;
			}

			if (isMihomoProfile && method === "GET") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				jsonResponse(
					route,
					normalizeMockStoredMihomoProfile(state.userMihomoProfiles[userId]),
				);
				return;
			}

			if (isMihomoProfile && method === "PUT") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				const payload = parseJsonBody(request);
				const normalized = normalizeMockMihomoProfilePayload(payload);
				if (!normalized.ok) {
					errorResponse(route, normalized.message, 400);
					return;
				}
				state.userMihomoProfiles[userId] = normalized.profile;
				jsonResponse(route, normalized.profile);
				return;
			}

			if (method === "GET") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				jsonResponse(route, user);
				return;
			}

			if (method === "PATCH") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				const payload = parseJsonBody(request);
				if (typeof payload.display_name === "string") {
					user.display_name = payload.display_name;
				}
				if (
					payload.priority_tier === "p1" ||
					payload.priority_tier === "p2" ||
					payload.priority_tier === "p3"
				) {
					user.priority_tier = payload.priority_tier;
				}
				if (payload.quota_reset) {
					user.quota_reset = payload.quota_reset as UserQuotaReset;
				}
				jsonResponse(route, user);
				return;
			}

			if (method === "DELETE") {
				state.users = state.users.filter((item) => item.user_id !== userId);
				state.nodeQuotas = state.nodeQuotas.filter((q) => q.user_id !== userId);
				delete state.userAccessByUserId[userId];
				delete state.userMihomoProfiles[userId];
				void route.fulfill({ status: 204, body: "" });
				return;
			}
		}

		if (path.startsWith("/api/sub/") && method === "GET") {
			const format = url.searchParams.get("format");
			if (format === "clash" || format === "mihomo") {
				textResponse(route, state.subscriptionContentClash);
				return;
			}
			textResponse(route, state.subscriptionContentRaw);
			return;
		}

		errorResponse(route, `${method} ${path} not mocked`);
	});

	return state;
}

export async function setAdminToken(
	page: Page,
	token = "test-token",
): Promise<void> {
	await page.addInitScript((value) => {
		window.localStorage.setItem("xp_admin_token", value as string);
	}, token);
}

export async function stubClipboard(page: Page): Promise<void> {
	await page.addInitScript(() => {
		// @ts-expect-error -- test-only helper
		window.__xp_clipboard_last_write = "";
		const clipboard = {
			writeText: async (text: string) => {
				// @ts-expect-error -- test-only helper
				window.__xp_clipboard_last_write = text;
			},
		};
		Object.defineProperty(navigator, "clipboard", {
			value: clipboard,
			configurable: true,
		});
	});
}
