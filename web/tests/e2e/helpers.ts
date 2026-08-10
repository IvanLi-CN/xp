import type { Page, Route } from "@playwright/test";
import yaml from "js-yaml";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

import { handleAdminConfigAndEndpointRoutes } from "./adminEndpointRouteMocks";
import { apiCapabilitiesFixture } from "./apiCapabilities";
import {
	type AdminEndpoint,
	type AdminNode,
	type AdminUser,
	type AdminUserNodeQuota,
	type NodeQuotaReset,
	type UserQuotaReset,
	applyFixtureUserPatch,
	buildFixtureUserAccessItem,
	buildFixtureUserNodeWeightItem,
	hasFixtureNodeQuotaReset,
	normalizeFixtureEndpoint,
	normalizeFixtureNode,
	normalizeFixtureQuota,
	normalizeFixtureQuotaLimit,
	normalizeFixtureUser,
} from "./fixtureStateSanitizers";

const { load } = yaml;

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
	xp_version: string;
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
	extra_proxies_yaml: string;
	extra_proxy_providers_yaml: string;
};

type MockApiOptions = {
	users?: AdminUser[];
	nodes?: AdminNode[];
	endpoints?: AdminEndpoint[];
	nodeQuotas?: AdminUserNodeQuota[];
	healthStatus?: "ok" | "error";
	mockStatusEvents?: boolean;
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
	nextEndpointId: () => string;
	nextEndpointTag: () => string;
	nextUserId: () => string;
};

function catalogMihomoProfile(): MockMihomoProfile {
	return {
		mixin_yaml: fixtureCatalog.string.none(),
		extra_proxies_yaml: fixtureCatalog.string.none(),
		extra_proxy_providers_yaml: fixtureCatalog.string.none(),
	};
}

function buildNodeRuntimeListItem(node: AdminNode) {
	return {
		node_id: node.node_id,
		node_name: node.node_name,
		api_base_url: node.api_base_url,
		access_host: node.access_host,
		summary: {
			status: "up",
			updated_at: fixtureCatalog.timestamp.t20260301T000000(),
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
				slot_start: fixtureCatalog.timestamp.t20260301T000000(),
				status: "up",
			},
		],
	};
}

function buildStatusHelloResponse(clusterInfo: ClusterInfo) {
	return {
		node_id: clusterInfo.node_id,
		connected_at: fixtureCatalog.timestamp.t20260301T000000(),
	};
}

const defaultNodes: AdminNode[] = [
	{
		node_id: fixtureCatalog.nodeId.fixture32(),
		node_name: fixtureCatalog.nodeName.fixture86(),
		api_base_url: fixtureCatalog.service.fixture87(),
		access_host: fixtureCatalog.host.fixture88(),
		quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
		quota_reset: fixtureCatalog.quota.reset() as NodeQuotaReset,
	},
];
const defaultEndpoints: AdminEndpoint[] = [
	{
		endpoint_id: fixtureCatalog.endpointId.fixture40(),
		node_id: fixtureCatalog.nodeId.fixture32(),
		tag: fixtureCatalog.endpointTag.fixture89(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: fixtureCatalog.endpoint.port443(),
		meta: {},
	},
];
const defaultUsers: AdminUser[] = [
	{
		user_id: fixtureCatalog.identifier.userPrimary(),
		display_name: "Demo user",
		subscription_token: fixtureCatalog.token.fixture90(),
		credential_epoch: fixtureCatalog.user.credentialEpoch(),
		priority_tier: fixtureCatalog.user.priorityTierDefault(),
		quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
	},
];
const defaultClusterInfo: ClusterInfo = {
	cluster_id: fixtureCatalog.cluster.fixture84(),
	node_id: fixtureCatalog.nodeId.fixture32(),
	role: "leader",
	leader_api_base_url: fixtureCatalog.service.fixture87(),
	term: 1,
	xp_version: "v0.1.0",
};
const defaultAlerts: AlertsResponse = {
	partial: false,
	unreachable_nodes: [],
	items: [],
};
const defaultSubscriptionClash = fixtureCatalog.subscription.clash();

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

function sseResponse(route: Route, payload: string, status = 200): void {
	void route.fulfill({
		status,
		contentType: "text/event-stream",
		headers: {
			"cache-control": "no-cache",
			connection: "keep-alive",
		},
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

type MockMihomoProfileError = { ok: false; message: string };

type MockMihomoProfileNormalizationResult =
	| { ok: true; profile: CanonicalMockMihomoProfile }
	| MockMihomoProfileError;

type ParsedYamlSequenceField =
	| { ok: true; value: unknown[] }
	| MockMihomoProfileError;

type ParsedYamlMappingField =
	| { ok: true; value: Record<string, unknown> }
	| MockMihomoProfileError;

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
				: fixtureCatalog.string.none(),
		extra_proxies_yaml:
			typeof profile?.extra_proxies_yaml === "string"
				? profile.extra_proxies_yaml
				: fixtureCatalog.string.none(),
		extra_proxy_providers_yaml:
			typeof profile?.extra_proxy_providers_yaml === "string"
				? profile.extra_proxy_providers_yaml
				: fixtureCatalog.string.none(),
	};
}

function parseYamlSequenceField(
	raw: string,
	fieldName: string,
): ParsedYamlSequenceField {
	if (raw.trim() === fixtureCatalog.string.none()) {
		return { ok: true, value: [] };
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
	return { ok: true, value };
}

function parseYamlMappingField(
	raw: string,
	fieldName: string,
): ParsedYamlMappingField {
	if (raw.trim() === fixtureCatalog.string.none()) {
		return { ok: true, value: {} };
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
	return { ok: true, value };
}

export function normalizeMockMihomoProfilePayload(
	payload: Record<string, unknown>,
): MockMihomoProfileNormalizationResult {
	if (Object.prototype.hasOwnProperty.call(payload, "template_yaml")) {
		return { ok: false, message: "template_yaml is no longer supported" };
	}
	const canonical = canonicalizeMockMihomoProfile(payload);
	if (canonical.mixin_yaml.trim() === fixtureCatalog.string.none()) {
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

	const mixinMap = mixinRoot;
	if (
		Object.prototype.hasOwnProperty.call(mixinMap, "proxies") &&
		canonical.extra_proxies_yaml.trim() !== fixtureCatalog.string.none()
	) {
		return {
			ok: false,
			message: "mixin_yaml.proxies cannot be combined with extra_proxies_yaml",
		};
	}
	if (
		Object.prototype.hasOwnProperty.call(mixinMap, "proxy-providers") &&
		canonical.extra_proxy_providers_yaml.trim() !== fixtureCatalog.string.none()
	) {
		return {
			ok: false,
			message:
				"mixin_yaml.proxy-providers cannot be combined with extra_proxy_providers_yaml",
		};
	}

	const extraProxies = parseYamlSequenceField(
		canonical.extra_proxies_yaml,
		"extra_proxies_yaml",
	);
	if (!extraProxies.ok) {
		return extraProxies;
	}
	const extraProxyProviders = parseYamlMappingField(
		canonical.extra_proxy_providers_yaml,
		"extra_proxy_providers_yaml",
	);
	if (!extraProxyProviders.ok) {
		return extraProxyProviders;
	}

	return {
		ok: true,
		profile: canonical,
	};
}
export function normalizeMockStoredMihomoProfile(
	profile: MockMihomoProfile | undefined,
): CanonicalMockMihomoProfile {
	return canonicalizeMockMihomoProfile(profile);
}
export async function setupApiMocks(
	page: Page,
	options: MockApiOptions = {},
): Promise<MockState> {
	const nextSubscriptionToken =
		fixtureCatalog.identifier.createSubscriptionTokenFactory();
	const nextEndpointId = fixtureCatalog.identifier.createEndpointIdFactory();
	const nextEndpointTag = fixtureCatalog.identifier.createEndpointTagFactory();
	const nextUserId = fixtureCatalog.identifier.createUserIdFactory();
	const users = (options.users ?? defaultUsers).map(normalizeFixtureUser);
	const nodes = (options.nodes ?? defaultNodes).map(normalizeFixtureNode);
	const endpoints = (options.endpoints ?? defaultEndpoints).map(
		normalizeFixtureEndpoint,
	);
	const nodeQuotas = (options.nodeQuotas ?? []).map(normalizeFixtureQuota);
	const state: MockState = {
		users,
		nodes,
		endpoints,
		nodeQuotas,
		userNodeWeights: {
			[fixtureCatalog.identifier.userPrimary()]: [
				{
					node_id: fixtureCatalog.identifier.nodePrimary(),
					weight: fixtureCatalog.number.value100(),
				},
			],
		},
		userAccessByUserId: {
			[fixtureCatalog.identifier.userPrimary()]: [
				{
					user_id: fixtureCatalog.identifier.userPrimary(),
					endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
					node_id: fixtureCatalog.identifier.nodePrimary(),
				},
			],
		},
		clusterInfo: { ...defaultClusterInfo },
		alerts: { ...defaultAlerts },
		healthStatus: options.healthStatus ?? "ok",
		subscriptionContentRaw: fixtureCatalog.subscription.rawUri(),
		subscriptionContentClash: defaultSubscriptionClash,
		userMihomoProfiles: {
			[fixtureCatalog.identifier.userPrimary()]: catalogMihomoProfile(),
		},
		nextEndpointId,
		nextEndpointTag,
		nextUserId,
	};
	await page.route("**/api/**", async (route) => {
		const request = route.request();
		const url = new URL(request.url());
		const path = url.pathname;
		const method = request.method();
		if (!path.startsWith("/api/")) return route.continue();
		if (path === "/api/health" && method === "GET") {
			jsonResponse(route, { status: state.healthStatus });
			return;
		}

		if (path === "/api/capabilities" && method === "GET") {
			jsonResponse(route, apiCapabilitiesFixture);
			return;
		}
		if (path === "/api/cluster/info" && method === "GET") {
			jsonResponse(route, state.clusterInfo);
			return;
		}

		if (path === "/api/version/check" && method === "GET") {
			jsonResponse(route, {
				current: {
					package: "0.1.0",
					release_tag: "v0.1.0",
				},
				latest: {
					release_tag: "v0.1.0",
					published_at: fixtureCatalog.timestamp.t20260301T000000(),
				},
				has_update: false,
				checked_at: fixtureCatalog.timestamp.t20260301T000000(),
				compare_reason: "up_to_date",
				source: {
					kind: "github_release",
					repo: "IvanLi-CN/xp",
					api_base: "https://api.github.com",
					channel: "stable",
				},
			});
			return;
		}

		if (
			handleAdminConfigAndEndpointRoutes({
				path,
				method,
				route,
				request,
				state,
			})
		) {
			return;
		}

		if (path === "/api/admin/alerts" && method === "GET") {
			jsonResponse(route, state.alerts);
			return;
		}

		if (path === "/api/admin/upgrade/status" && method === "GET") {
			jsonResponse(route, {
				support: {
					supported: true,
					reason: null,
					trigger: "systemd",
				},
				status: {
					state: "idle",
					target_tag: null,
					repo: "IvanLi-CN/xp",
					started_at: null,
					finished_at: null,
					exit_code: null,
					message: null,
					updated_at: fixtureCatalog.timestamp.t20260301T000000(),
				},
			});
			return;
		}

		if (path === "/api/admin/nodes" && method === "GET") {
			jsonResponse(route, { items: state.nodes });
			return;
		}

		if (path === "/api/admin/nodes/runtime" && method === "GET") {
			const items = state.nodes.map(buildNodeRuntimeListItem);
			jsonResponse(route, {
				partial: false,
				unreachable_nodes: [],
				items,
			});
			return;
		}

		if (
			path === "/api/admin/status/events" &&
			method === "GET" &&
			options.mockStatusEvents !== false
		) {
			const items = state.nodes.map(buildNodeRuntimeListItem);
			const payload = [
				`event: hello\ndata: ${JSON.stringify(buildStatusHelloResponse(state.clusterInfo))}\n`,
				`event: snapshot\ndata: ${JSON.stringify({
					emitted_at: fixtureCatalog.timestamp.t20260301T000000(),
					health: { status: "ok" },
					cluster_info: state.clusterInfo,
					nodes_runtime: {
						partial: false,
						unreachable_nodes: [],
						items,
					},
					alerts: state.alerts,
					upgrade: {
						support: {
							supported: true,
							reason: null,
							trigger: "systemd",
						},
						status: {
							state: "idle",
							target_tag: null,
							repo: "IvanLi-CN/xp",
							started_at: null,
							finished_at: null,
							exit_code: null,
							message: null,
							updated_at: fixtureCatalog.timestamp.t20260301T000000(),
						},
					},
				})}\n`,
			]
				.map((event) => `${event}\n`)
				.join(fixtureCatalog.string.none());
			sseResponse(route, payload);
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
			const requestBody = parseJsonBody(request);
			const quotaLimit = normalizeFixtureQuotaLimit(
				requestBody.quota_limit_bytes,
			);
			if (quotaLimit === fixtureCatalog.quota.usedBytes()) {
				node.quota_limit_bytes = fixtureCatalog.quota.usedBytes();
			} else if (quotaLimit === fixtureCatalog.quota.limitBytes()) {
				node.quota_limit_bytes = fixtureCatalog.quota.limitBytes();
			} else if (quotaLimit === fixtureCatalog.quota.oneGiB()) {
				node.quota_limit_bytes = fixtureCatalog.quota.oneGiB();
			} else if (quotaLimit === fixtureCatalog.quota.fiveGiB()) {
				node.quota_limit_bytes = fixtureCatalog.quota.fiveGiB();
			} else if (quotaLimit === fixtureCatalog.quota.tenGiB()) {
				node.quota_limit_bytes = fixtureCatalog.quota.tenGiB();
			}
			if (hasFixtureNodeQuotaReset(requestBody.quota_reset)) {
				node.quota_reset = fixtureCatalog.quota.reset() as NodeQuotaReset;
			}
			jsonResponse(route, node);
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
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					used_bytes: fixtureCatalog.quota.usedBytes(),
					remaining_bytes: fixtureCatalog.quota.usedBytes(),
				})),
			});
			return;
		}

		if (path === "/api/admin/users" && method === "POST") {
			const payload = parseJsonBody(request);
			const displayName =
				typeof payload.display_name === "string"
					? payload.display_name
					: "Fixture user";
			const userId = state.nextUserId();
			const newUser: AdminUser = {
				user_id: userId,
				display_name: displayName,
				subscription_token: nextSubscriptionToken(),
				credential_epoch: fixtureCatalog.user.credentialEpoch(),
				priority_tier: fixtureCatalog.user.priorityTierDefault(),
				quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
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
					typeof item.endpoint_id === "string"
						? item.endpoint_id
						: fixtureCatalog.string.none();
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
				.map((endpointId) => {
					const endpoint = endpointById.get(endpointId);
					if (!endpoint) throw new Error(`missing endpoint: ${endpointId}`);
					return buildFixtureUserAccessItem(userId, endpoint);
				});
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
			const userId = decodeURIComponent(
				segments[4] ?? fixtureCatalog.string.none(),
			);
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
				const requestBody = parseJsonBody(request);
				const rawWeight = requestBody.weight;
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
				const next: AdminUserNodeWeightItem = buildFixtureUserNodeWeightItem(
					node,
					rawWeight,
				);
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
				user.subscription_token = nextSubscriptionToken();
				jsonResponse(route, {
					subscription_token: user.subscription_token,
				});
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
				const profile = catalogMihomoProfile();
				state.userMihomoProfiles[userId] = profile;
				jsonResponse(route, profile);
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
				applyFixtureUserPatch(user, payload);
				jsonResponse(route, user);
				return;
			}

			if (method === "DELETE") {
				state.users = state.users.filter((item) => item.user_id !== userId);
				state.nodeQuotas = state.nodeQuotas.filter((q) => q.user_id !== userId);
				delete state.userAccessByUserId[userId];
				delete state.userMihomoProfiles[userId];
				void route.fulfill({
					status: 204,
					body: fixtureCatalog.string.none(),
				});
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
		window.__xp_clipboard_last_write = fixtureCatalog.string.none();
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
