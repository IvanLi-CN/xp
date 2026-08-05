export const API_COMPATIBILITY_WINDOW = ["3.22", "3.21", "3.20"] as const;

export type ApiCompatibilityMinor = (typeof API_COMPATIBILITY_WINDOW)[number];

export const API_CAPABILITIES = [
	"api.health",
	"api.cluster-info",
	"admin.nodes",
	"admin.users",
	"admin.endpoints",
	"admin.quota-policy",
	"admin.status-events",
	"admin.upgrade",
	"admin.mesh",
	"admin.reality-domains",
	"admin.node-probes",
	"admin.traffic-usage",
	"admin.mihomo-tools",
] as const;

export type ApiCapability = (typeof API_CAPABILITIES)[number] | (string & {});

export type ReleaseInventory = {
	minor: ApiCompatibilityMinor;
	releaseTag: `v${string}`;
	sourceCommit: string;
	apiRoutes: readonly string[];
	webCallsites: readonly string[];
	responseSchemas: Readonly<Record<string, readonly string[]>>;
	capabilities: readonly ApiCapability[];
	fingerprint: Readonly<Record<string, readonly string[]>>;
};

export type ReleaseCompatibilityContract = {
	direction: "new-web-to-legacy-api" | "legacy-web-to-new-api";
	consumerReleaseTag: string;
	serverReleaseTag: string;
	serverMinor: ApiCompatibilityMinor;
	apiRoutes: readonly string[];
	webCallsites: readonly string[];
	responseSchemas: Readonly<Record<string, readonly string[]>>;
};

export type ReleaseApiFixture = {
	releaseTag: string;
	sourceCommit: string;
	requests: readonly {
		method: "DELETE" | "GET" | "PATCH" | "POST" | "PUT";
		path: string;
		expectedStatus: 200;
	}[];
	responses: readonly {
		route: string;
		contentType: "application/json" | "text/plain";
		body: unknown;
	}[];
};

const API_ROUTES_322 = [
	"GET /api/health",
	"GET /api/cluster/info",
	"GET /api/capabilities",
	"GET /api/version/check",
	"POST /api/admin/cluster/join-tokens",
	"GET /api/admin/alerts",
	"GET /api/admin/config",
	"GET /api/admin/mihomo/resource-policy",
	"PUT /api/admin/mihomo/resource-policy",
	"GET /api/admin/nodes",
	"GET /api/admin/nodes/{node_id}",
	"PATCH /api/admin/nodes/{node_id}",
	"GET /api/admin/nodes/{node_id}/delete-preview",
	"POST /api/admin/nodes/{node_id}/egress-probe/refresh",
	"GET /api/admin/nodes/runtime",
	"GET /api/admin/nodes/{node_id}/runtime",
	"GET /api/admin/nodes/{node_id}/history",
	"GET /api/admin/nodes/{node_id}/traffic",
	"GET /api/admin/nodes/{node_id}/ip-usage",
	"GET /api/admin/nodes/{node_id}/tcp-connections",
	"GET /api/admin/nodes/{node_id}/runtime/events",
	"GET /api/admin/status/events",
	"GET /api/admin/mesh/status",
	"POST /api/admin/mesh/probes",
	"GET /api/admin/endpoints",
	"POST /api/admin/endpoints",
	"GET /api/admin/endpoints/{endpoint_id}",
	"PATCH /api/admin/endpoints/{endpoint_id}",
	"DELETE /api/admin/endpoints/{endpoint_id}",
	"POST /api/admin/endpoints/{endpoint_id}/rotate-shortid",
	"POST /api/admin/endpoints/{endpoint_id}/canary-probe",
	"GET /api/admin/endpoints/{endpoint_id}/probe-history",
	"POST /api/admin/endpoints/probe/run",
	"GET /api/admin/endpoints/probe/runs/{run_id}",
	"GET /api/admin/endpoints/probe/runs/{run_id}/events",
	"GET /api/admin/reality-domains",
	"POST /api/admin/reality-domains",
	"PATCH /api/admin/reality-domains/{domain_id}",
	"DELETE /api/admin/reality-domains/{domain_id}",
	"POST /api/admin/reality-domains/reorder",
	"GET /api/admin/users",
	"POST /api/admin/users",
	"GET /api/admin/users/{user_id}",
	"PATCH /api/admin/users/{user_id}",
	"DELETE /api/admin/users/{user_id}",
	"POST /api/admin/users/{user_id}/reset-token",
	"POST /api/admin/users/{user_id}/reset-credentials",
	"GET /api/admin/users/{user_id}/access",
	"PUT /api/admin/users/{user_id}/access",
	"GET /api/admin/users/{user_id}/node-quotas",
	"PUT /api/admin/users/{user_id}/node-quotas/{node_id}",
	"GET /api/admin/users/{user_id}/node-quotas/status",
	"GET /api/admin/users/{user_id}/node-weights",
	"PUT /api/admin/users/{user_id}/node-weights/{node_id}",
	"GET /api/admin/users/{user_id}/ip-usage",
	"GET /api/admin/users/{user_id}/traffic",
	"GET /api/admin/users/{user_id}/subscription-mihomo-profile",
	"PUT /api/admin/users/{user_id}/subscription-mihomo-profile",
	"GET /api/admin/users/quota-summaries",
	"GET /api/admin/quota-policy/global-weight-rows",
	"PUT /api/admin/quota-policy/global-weight-rows/{user_id}",
	"GET /api/admin/quota-policy/nodes/{node_id}/weight-rows",
	"GET /api/admin/quota-policy/nodes/{node_id}/policy",
	"PUT /api/admin/quota-policy/nodes/{node_id}/policy",
	"GET /api/admin/upgrade/status",
	"POST /api/admin/upgrade/start",
	"POST /api/admin/tools/mihomo/redact",
	"GET /api/admin/nodes/{node_id}/history",
	"GET /api/admin/nodes/{node_id}/traffic",
	"GET /api/admin/nodes/{node_id}/ip-usage",
	"GET /api/admin/nodes/{node_id}/tcp-connections",
	"GET /api/sub/{subscription_token}",
	"GET /events",
] as const;

const API_ROUTES_320_321 = API_ROUTES_322.filter(
	(route) =>
		route !== "GET /api/capabilities" &&
		route !== "GET /api/admin/mesh/status" &&
		route !== "POST /api/admin/mesh/probes",
);

const WEB_CALLSITES_322 = [
	"adminAlerts",
	"adminAuth",
	"adminConfig",
	"adminEndpointProbes",
	"adminEndpoints",
	"adminJoinTokens",
	"adminIpUsage",
	"adminMesh",
	"adminNodeHistory",
	"adminNodeRuntime",
	"adminNodes",
	"adminQuotaPolicyGlobalWeightRows",
	"adminQuotaPolicyNodePolicy",
	"adminQuotaPolicyNodeWeightRows",
	"adminRealityDomains",
	"adminStatusEvents",
	"adminTcpConnections",
	"adminTools",
	"adminTraffic",
	"adminUpgrade",
	"adminUserAccess",
	"adminUserNodeQuotaStatus",
	"adminUserNodeQuotas",
	"adminUserNodeWeights",
	"adminUserQuotaSummaries",
	"adminUsers",
	"clusterInfo",
	"health",
	"sse",
	"subscription",
	"versionCheck",
] as const;

const WEB_CALLSITES_320_321 = WEB_CALLSITES_322.filter(
	(callsite) => callsite !== "adminMesh",
);

const RESPONSE_SCHEMAS_320 = {
	"GET /api/health": ["status"],
	"GET /api/cluster/info": [
		"cluster_id",
		"node_id",
		"role",
		"leader_api_base_url",
		"term",
		"xp_version",
	],
	"GET /api/admin/nodes": ["items"],
	"GET /api/admin/nodes/runtime": ["partial", "unreachable_nodes", "items"],
	"GET /api/admin/status/events": ["hello", "snapshot"],
	"GET /api/version/check": ["current", "latest", "has_update", "checked_at"],
} as const;

const RESPONSE_SCHEMAS_321 = {
	...RESPONSE_SCHEMAS_320,
	"GET /api/admin/config": [
		"bind",
		"xray_api_addr",
		"data_dir",
		"node_name",
		"access_host",
		"api_base_url",
		"vless_https_canary_bind",
		"quota_poll_interval_secs",
		"quota_auto_unban",
		"ip_geo_enabled",
		"ip_geo_origin",
		"admin_token_present",
		"admin_token_masked",
	],
	"GET /api/sub/{subscription_token}": ["text/plain"],
} as const;

const RESPONSE_SCHEMAS_322 = {
	...RESPONSE_SCHEMAS_321,
	"GET /api/admin/status/events": ["hello", "snapshot", "mesh_revision"],
	"GET /api/admin/mesh/status": [
		"generated_at",
		"revision",
		"local",
		"peers",
		"events",
	],
	"GET /api/capabilities": ["release_tag", "capabilities", "fingerprint"],
} as const;

const FINGERPRINT_320_321 = {
	"/api/health": ["status"],
	"/api/cluster/info": [
		"cluster_id",
		"node_id",
		"role",
		"leader_api_base_url",
		"term",
	],
	"/api/admin/nodes": ["items"],
} as const;

const FINGERPRINT_322 = {
	...FINGERPRINT_320_321,
	"/api/admin/mesh/status": [
		"generated_at",
		"revision",
		"local",
		"peers",
		"events",
	],
} as const;

export const CURRENT_API_FINGERPRINT = {
	"/api/health": ["status"],
	"/api/cluster/info": [
		"cluster_id",
		"node_id",
		"role",
		"leader_api_base_url",
		"term",
	],
	"/api/admin/nodes": ["items"],
	"/api/admin/status/events": ["hello", "snapshot"],
} as const;

const COMMON_CAPABILITIES = [
	"api.health",
	"api.cluster-info",
	"admin.nodes",
	"admin.users",
	"admin.endpoints",
	"admin.quota-policy",
	"admin.status-events",
	"admin.upgrade",
	"admin.traffic-usage",
	"admin.mihomo-tools",
] as const satisfies readonly ApiCapability[];

export const RELEASE_INVENTORIES: readonly ReleaseInventory[] = [
	{
		minor: "3.22",
		releaseTag: "v3.22.5",
		sourceCommit: "d7e1e652fbd5fa07442bd960894764a5b81ef3bc",
		apiRoutes: API_ROUTES_322,
		webCallsites: WEB_CALLSITES_322,
		responseSchemas: RESPONSE_SCHEMAS_322,
		capabilities: [
			...COMMON_CAPABILITIES,
			"admin.mesh",
			"admin.reality-domains",
			"admin.node-probes",
		],
		fingerprint: FINGERPRINT_322,
	},
	{
		minor: "3.21",
		releaseTag: "v3.21.11",
		sourceCommit: "8cf9564f366cb260b35106396840cbe3ed903c75",
		apiRoutes: API_ROUTES_320_321,
		webCallsites: WEB_CALLSITES_320_321,
		responseSchemas: RESPONSE_SCHEMAS_321,
		capabilities: COMMON_CAPABILITIES,
		fingerprint: FINGERPRINT_320_321,
	},
	{
		minor: "3.20",
		releaseTag: "v3.20.3",
		sourceCommit: "aee855e41f63af0c99f296259b038cda73e24ae9",
		apiRoutes: API_ROUTES_320_321,
		webCallsites: WEB_CALLSITES_320_321,
		responseSchemas: RESPONSE_SCHEMAS_320,
		capabilities: COMMON_CAPABILITIES,
		fingerprint: FINGERPRINT_320_321,
	},
] as const;

export const RELEASE_COMPATIBILITY_CONTRACTS: readonly ReleaseCompatibilityContract[] =
	RELEASE_INVENTORIES.flatMap((inventory) => [
		{
			direction: "new-web-to-legacy-api" as const,
			consumerReleaseTag: "current",
			serverReleaseTag: inventory.releaseTag,
			serverMinor: inventory.minor,
			apiRoutes: inventory.apiRoutes,
			webCallsites: inventory.webCallsites,
			responseSchemas: inventory.responseSchemas,
		},
		{
			direction: "legacy-web-to-new-api" as const,
			consumerReleaseTag: inventory.releaseTag,
			serverReleaseTag: "v3.22.5",
			serverMinor: "3.22" as const,
			apiRoutes: inventory.apiRoutes,
			webCallsites: inventory.webCallsites,
			responseSchemas: inventory.responseSchemas,
		},
	]);

const FIXTURE_NODE = {
	node_id: "01fixture000000000000000000",
	node_name: "fixture-node",
	api_base_url: "https://fixture.example/api",
	access_host: "fixture.example",
	quota_limit_bytes: 0,
	quota_reset: { policy: "unlimited", tz_offset_minutes: 0 },
} as const;

const FIXTURE_RESPONSES = {
	"GET /api/health": {
		contentType: "application/json" as const,
		body: { status: "ok", extra_field_added_by_server: true },
	},
	"GET /api/cluster/info": {
		contentType: "application/json" as const,
		body: {
			cluster_id: "fixture-cluster",
			node_id: FIXTURE_NODE.node_id,
			role: "leader",
			leader_api_base_url: "https://fixture.example",
			term: 7,
			xp_version: "3.22.5",
			extra_field_added_by_server: true,
		},
	},
	"GET /api/admin/nodes": {
		contentType: "application/json" as const,
		body: { items: [FIXTURE_NODE], extra_field_added_by_server: true },
	},
	"GET /api/admin/nodes/runtime": {
		contentType: "application/json" as const,
		body: { partial: false, unreachable_nodes: [], items: [] },
	},
	"GET /api/admin/status/events": {
		contentType: "application/json" as const,
		body: {
			hello: {
				node_id: FIXTURE_NODE.node_id,
				connected_at: "2026-08-05T00:00:00Z",
			},
			snapshot: {
				emitted_at: "2026-08-05T00:00:00Z",
				health: { status: "ok" },
				cluster_info: {
					cluster_id: "fixture-cluster",
					node_id: FIXTURE_NODE.node_id,
					role: "leader",
					leader_api_base_url: "https://fixture.example",
					term: 7,
					xp_version: "3.22.5",
				},
				nodes_runtime: { partial: false, unreachable_nodes: [], items: [] },
				alerts: { partial: false, unreachable_nodes: [], items: [] },
				upgrade: {
					support: { supported: false },
					status: { state: "idle", updated_at: "2026-08-05T00:00:00Z" },
				},
				mesh_revision: 11,
			},
		},
	},
	"GET /api/version/check": {
		contentType: "application/json" as const,
		body: {
			current: { package: "xp", release_tag: "v3.22.5" },
			latest: { release_tag: "v3.22.5", published_at: "2026-08-05T00:00:00Z" },
			has_update: false,
			checked_at: "2026-08-05T00:00:00Z",
			compare_reason: "fixture",
			source: {
				kind: "github",
				repo: "IvanLi-CN/xp",
				api_base: "https://api.github.com",
				channel: "stable",
			},
		},
	},
	"GET /api/admin/config": {
		contentType: "application/json" as const,
		body: {
			bind: "127.0.0.1:62416",
			xray_api_addr: "127.0.0.1:10085",
			data_dir: "/var/lib/xp",
			node_name: "fixture-node",
			access_host: "fixture.example",
			api_base_url: "https://fixture.example",
			vless_https_canary_bind: "127.0.0.1:18080",
			quota_poll_interval_secs: 60,
			quota_auto_unban: false,
			ip_geo_enabled: false,
			ip_geo_origin: "",
			admin_token_present: true,
			admin_token_masked: "****",
		},
	},
	"GET /api/admin/mesh/status": {
		contentType: "application/json" as const,
		body: {
			generated_at: "2026-08-05T00:00:00Z",
			revision: 11,
			local: {},
			peers: [],
			events: [],
		},
	},
	"GET /api/capabilities": {
		contentType: "application/json" as const,
		body: {
			release_tag: "v3.22.5",
			capabilities: ["api.health", "admin.nodes"],
			fingerprint: { "/api/health": ["status"] },
		},
	},
	"GET /api/sub/{subscription_token}": {
		contentType: "text/plain" as const,
		body: "proxy-provider-fixture",
	},
} as const;

function fixtureRequests(
	inventory: ReleaseInventory,
): ReleaseApiFixture["requests"] {
	return inventory.apiRoutes.map((route) => {
		const separator = route.indexOf(" ");
		return {
			method: route.slice(
				0,
				separator,
			) as ReleaseApiFixture["requests"][number]["method"],
			path: route.slice(separator + 1),
			expectedStatus: 200,
		};
	});
}

function fixtureResponses(
	inventory: ReleaseInventory,
): ReleaseApiFixture["responses"] {
	return Object.entries(inventory.responseSchemas).flatMap(([route]) => {
		const response = FIXTURE_RESPONSES[route as keyof typeof FIXTURE_RESPONSES];
		return response ? [{ route, ...response }] : [];
	});
}

export const RELEASE_API_FIXTURES: readonly ReleaseApiFixture[] =
	RELEASE_INVENTORIES.map((inventory) => ({
		releaseTag: inventory.releaseTag,
		sourceCommit: inventory.sourceCommit,
		requests: fixtureRequests(inventory),
		responses: fixtureResponses(inventory),
	}));
