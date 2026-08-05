import {
	type ReleaseFixtureResponse,
	statusEventFixture,
} from "./releaseInventoryFixtures";

export const API_COMPATIBILITY_WINDOW = ["3.22", "3.21", "3.20"] as const;
export type ApiCompatibilityMinor = (typeof API_COMPATIBILITY_WINDOW)[number];
export const API_CAPABILITIES = [
	"api.health",
	"api.cluster-info",
	"admin.nodes",
	"admin.users",
	"admin.endpoints",
	"admin.alerts",
	"admin.config",
	"admin.quota-policy",
	"admin.status-events",
	"admin.upgrade",
	"admin.mesh",
	"admin.reality-domains",
	"admin.node-probes",
	"admin.traffic-usage",
	"admin.mihomo-tools",
	"admin.mihomo-resource-policy",
] as const;

export const API_CAPABILITIES_PATH = "/api/capabilities";
export type ApiCapability = (typeof API_CAPABILITIES)[number] | (string & {});
export type ReleaseInventory = {
	minor: ApiCompatibilityMinor;
	releaseTag: `v${string}`;
	sourceCommit: string;
	capabilityProbePath?: string;
	apiRoutes: readonly string[];
	webCallsites: readonly string[];
	requestSchemas: Readonly<Record<string, readonly string[]>>;
	responseSchemas: Readonly<Record<string, readonly string[]>>;
	capabilities: readonly ApiCapability[];
	fingerprint: Readonly<Record<string, readonly string[]>>;
};
export type ReleaseCompatibilityContract = {
	direction: "new-web-to-legacy-api" | "legacy-web-to-new-api";
	consumerReleaseTag: string;
	serverReleaseTag: string;
	serverMinor: ApiCompatibilityMinor;
	consumerRoutes: readonly string[];
	serverRoutes: readonly string[];
	webCallsites: readonly string[];
	requestSchemas: Readonly<Record<string, readonly string[]>>;
	responseSchemas: Readonly<Record<string, readonly string[]>>;
};
export type ReleaseApiFixture = {
	direction: "new-web-to-legacy-api" | "legacy-web-to-new-api";
	consumerReleaseTag: string;
	serverReleaseTag: string;
	consumerSourceCommit: string;
	serverSourceCommit: string;
	requests: readonly {
		route: string;
		method: "DELETE" | "GET" | "PATCH" | "POST" | "PUT";
		path: string;
		expectedStatus: 200 | 204 | 404;
		optional: boolean;
		requestSchema: readonly string[];
	}[];
	responses: readonly {
		route: string;
		fields: readonly string[];
		missingFields: readonly string[];
		contentType: "application/json" | "text/event-stream" | "text/plain";
		body: unknown;
		wireBody?: string;
	}[];
};
const API_ROUTES_322 = [
	"GET /api/health",
	"GET /api/cluster/info",
	"GET /api/version/check",
	"POST /api/cluster/join",
	"GET /api/sub/{subscription_token}/mihomo/provider",
	"GET /api/sub/{subscription_token}/mihomo/provider/system",
	"POST /api/admin/cluster/join-tokens",
	"GET /api/admin/alerts",
	"GET /api/admin/config",
	"GET /api/admin/mihomo/resource-policy",
	"PUT /api/admin/mihomo/resource-policy",
	"GET /api/admin/nodes",
	"GET /api/admin/nodes/{node_id}",
	"PATCH /api/admin/nodes/{node_id}",
	"DELETE /api/admin/nodes/{node_id}",
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
	"GET /api/sub/{subscription_token}",
] as const;
const API_ROUTES_321 = [
	"GET /api/health",
	"GET /api/cluster/info",
	"GET /api/version/check",
	"POST /api/cluster/join",
	"GET /api/sub/{subscription_token}/mihomo/provider",
	"GET /api/sub/{subscription_token}/mihomo/provider/system",
	"POST /api/admin/cluster/join-tokens",
	"GET /api/admin/alerts",
	"GET /api/admin/config",
	"GET /api/admin/nodes",
	"GET /api/admin/nodes/{node_id}",
	"PATCH /api/admin/nodes/{node_id}",
	"DELETE /api/admin/nodes/{node_id}",
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
	"GET /api/sub/{subscription_token}",
] as const;
const API_ROUTES_320 = [
	"GET /api/health",
	"GET /api/cluster/info",
	"GET /api/version/check",
	"POST /api/cluster/join",
	"GET /api/sub/{subscription_token}/mihomo/provider",
	"GET /api/sub/{subscription_token}/mihomo/provider/system",
	"POST /api/admin/cluster/join-tokens",
	"GET /api/admin/alerts",
	"GET /api/admin/config",
	"GET /api/admin/nodes",
	"GET /api/admin/nodes/{node_id}",
	"PATCH /api/admin/nodes/{node_id}",
	"DELETE /api/admin/nodes/{node_id}",
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
	"GET /api/sub/{subscription_token}",
] as const;
type ResponseSchemaMap = Readonly<Record<string, readonly string[]>>;

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

const WEB_CALLSITES_321 = [
	"adminAlerts",
	"adminAuth",
	"adminConfig",
	"adminEndpointProbes",
	"adminEndpoints",
	"adminJoinTokens",
	"adminIpUsage",
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

const WEB_CALLSITES_320 = [
	"adminAlerts",
	"adminAuth",
	"adminConfig",
	"adminEndpointProbes",
	"adminEndpoints",
	"adminJoinTokens",
	"adminIpUsage",
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

const RESPONSE_FIELDS_320 = {
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

const RESPONSE_FIELDS_321 = {
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

const RESPONSE_FIELDS_322 = {
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
	"GET /api/version/check": ["current", "latest", "has_update", "checked_at"],
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
		"mihomo_resource_allow_private_targets",
		"admin_token_present",
		"admin_token_masked",
	],
	"GET /api/admin/status/events": ["hello", "snapshot", "mesh_revision"],
	"GET /api/admin/mesh/status": [
		"generated_at",
		"revision",
		"local",
		"peers",
		"events",
	],
	"GET /api/sub/{subscription_token}": ["text/plain"],
} as const;

const REQUEST_SCHEMAS_320: ResponseSchemaMap = {
	"POST /api/admin/cluster/join-tokens": ["AdminJoinTokenRequest"],
	"PATCH /api/admin/nodes/{node_id}": ["AdminNodePatchRequest"],
	"POST /api/admin/endpoints": ["AdminEndpointCreateRequest"],
	"PATCH /api/admin/endpoints/{endpoint_id}": ["AdminEndpointPatchRequest"],
	"POST /api/admin/reality-domains": ["AdminRealityDomainCreateRequest"],
	"PATCH /api/admin/reality-domains/{domain_id}": [
		"AdminRealityDomainPatchRequest",
	],
	"POST /api/admin/reality-domains/reorder": ["domain_ids"],
	"POST /api/admin/users": ["AdminUserCreateRequest"],
	"PATCH /api/admin/users/{user_id}": ["AdminUserPatchRequest"],
	"PUT /api/admin/users/{user_id}/access": ["PutAdminUserAccessRequest"],
	"PUT /api/admin/users/{user_id}/node-quotas/{node_id}": [
		"quota_limit_bytes",
		"quota_reset_source",
	],
	"PUT /api/admin/users/{user_id}/node-weights/{node_id}": ["weight"],
	"PUT /api/admin/quota-policy/global-weight-rows/{user_id}": ["weight"],
	"PUT /api/admin/quota-policy/nodes/{node_id}/policy": ["inherit_global"],
	"POST /api/admin/tools/mihomo/redact": ["AdminMihomoRedactRequest"],
	"POST /api/admin/upgrade/start": ["target_tag"],
};

const REQUEST_SCHEMAS_321: ResponseSchemaMap = {
	"POST /api/admin/cluster/join-tokens": ["AdminJoinTokenRequest"],
	"PATCH /api/admin/nodes/{node_id}": ["AdminNodePatchRequest"],
	"POST /api/admin/endpoints": ["AdminEndpointCreateRequest"],
	"PATCH /api/admin/endpoints/{endpoint_id}": ["AdminEndpointPatchRequest"],
	"POST /api/admin/reality-domains": ["AdminRealityDomainCreateRequest"],
	"PATCH /api/admin/reality-domains/{domain_id}": [
		"AdminRealityDomainPatchRequest",
	],
	"POST /api/admin/reality-domains/reorder": ["domain_ids"],
	"POST /api/admin/users": ["AdminUserCreateRequest"],
	"PATCH /api/admin/users/{user_id}": ["AdminUserPatchRequest"],
	"PUT /api/admin/users/{user_id}/access": ["PutAdminUserAccessRequest"],
	"PUT /api/admin/users/{user_id}/node-quotas/{node_id}": [
		"quota_limit_bytes",
		"quota_reset_source",
	],
	"PUT /api/admin/users/{user_id}/node-weights/{node_id}": ["weight"],
	"PUT /api/admin/quota-policy/global-weight-rows/{user_id}": ["weight"],
	"PUT /api/admin/quota-policy/nodes/{node_id}/policy": ["inherit_global"],
	"POST /api/admin/tools/mihomo/redact": ["AdminMihomoRedactRequest"],
	"POST /api/admin/upgrade/start": ["target_tag"],
};

const REQUEST_SCHEMAS_322: ResponseSchemaMap = {
	"POST /api/admin/cluster/join-tokens": ["AdminJoinTokenRequest"],
	"PATCH /api/admin/nodes/{node_id}": ["AdminNodePatchRequest"],
	"POST /api/admin/endpoints": ["AdminEndpointCreateRequest"],
	"PATCH /api/admin/endpoints/{endpoint_id}": ["AdminEndpointPatchRequest"],
	"POST /api/admin/reality-domains": ["AdminRealityDomainCreateRequest"],
	"PATCH /api/admin/reality-domains/{domain_id}": [
		"AdminRealityDomainPatchRequest",
	],
	"POST /api/admin/reality-domains/reorder": ["domain_ids"],
	"POST /api/admin/users": ["AdminUserCreateRequest"],
	"PATCH /api/admin/users/{user_id}": ["AdminUserPatchRequest"],
	"PUT /api/admin/users/{user_id}/access": ["PutAdminUserAccessRequest"],
	"PUT /api/admin/users/{user_id}/node-quotas/{node_id}": [
		"quota_limit_bytes",
		"quota_reset_source",
	],
	"PUT /api/admin/users/{user_id}/node-weights/{node_id}": ["weight"],
	"PUT /api/admin/quota-policy/global-weight-rows/{user_id}": ["weight"],
	"PUT /api/admin/quota-policy/nodes/{node_id}/policy": ["inherit_global"],
	"POST /api/admin/tools/mihomo/redact": ["AdminMihomoRedactRequest"],
	"POST /api/admin/upgrade/start": ["target_tag"],
	"POST /api/admin/mesh/probes": ["node_ids"],
};

const RESPONSE_SCHEMAS_320: ResponseSchemaMap = RESPONSE_FIELDS_320;
const RESPONSE_SCHEMAS_321: ResponseSchemaMap = RESPONSE_FIELDS_321;
const RESPONSE_SCHEMAS_322: ResponseSchemaMap = RESPONSE_FIELDS_322;

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
	"/api/health": ["status"],
	"/api/cluster/info": [
		"cluster_id",
		"node_id",
		"role",
		"leader_api_base_url",
		"term",
	],
	"/api/admin/nodes": ["items"],
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
	"admin.alerts",
	"admin.config",
	"admin.quota-policy",
	"admin.status-events",
	"admin.upgrade",
	"admin.traffic-usage",
	"admin.mihomo-tools",
	"admin.reality-domains",
	"admin.node-probes",
] as const satisfies readonly ApiCapability[];

export const RELEASE_INVENTORIES: readonly ReleaseInventory[] = [
	{
		minor: "3.22",
		releaseTag: "v3.22.5",
		sourceCommit: "d7e1e652fbd5fa07442bd960894764a5b81ef3bc",
		capabilityProbePath: API_CAPABILITIES_PATH,
		apiRoutes: API_ROUTES_322,
		webCallsites: WEB_CALLSITES_322,
		requestSchemas: REQUEST_SCHEMAS_322,
		responseSchemas: RESPONSE_SCHEMAS_322,
		capabilities: [
			...COMMON_CAPABILITIES,
			"admin.mesh",
			"admin.mihomo-resource-policy",
		],
		fingerprint: FINGERPRINT_322,
	},
	{
		minor: "3.21",
		releaseTag: "v3.21.11",
		sourceCommit: "8cf9564f366cb260b35106396840cbe3ed903c75",
		apiRoutes: API_ROUTES_321,
		webCallsites: WEB_CALLSITES_321,
		requestSchemas: REQUEST_SCHEMAS_321,
		responseSchemas: RESPONSE_SCHEMAS_321,
		capabilities: COMMON_CAPABILITIES,
		fingerprint: FINGERPRINT_320_321,
	},
	{
		minor: "3.20",
		releaseTag: "v3.20.3",
		sourceCommit: "aee855e41f63af0c99f296259b038cda73e24ae9",
		apiRoutes: API_ROUTES_320,
		webCallsites: WEB_CALLSITES_320,
		requestSchemas: REQUEST_SCHEMAS_320,
		responseSchemas: RESPONSE_SCHEMAS_320,
		capabilities: COMMON_CAPABILITIES,
		fingerprint: FINGERPRINT_320_321,
	},
] as const;

export const RELEASE_COMPATIBILITY_CONTRACTS: readonly ReleaseCompatibilityContract[] =
	RELEASE_INVENTORIES.flatMap((serverInventory) => {
		const currentInventory = RELEASE_INVENTORIES[0];
		if (!currentInventory) return [];
		const legacyContracts: ReleaseCompatibilityContract[] = [
			{
				direction: "new-web-to-legacy-api",
				consumerReleaseTag: currentInventory.releaseTag,
				serverReleaseTag: serverInventory.releaseTag,
				serverMinor: serverInventory.minor,
				consumerRoutes: currentInventory.apiRoutes,
				serverRoutes: serverInventory.apiRoutes,
				webCallsites: currentInventory.webCallsites,
				requestSchemas: currentInventory.requestSchemas,
				responseSchemas: currentInventory.responseSchemas,
			},
			{
				direction: "legacy-web-to-new-api",
				consumerReleaseTag: serverInventory.releaseTag,
				serverReleaseTag: currentInventory.releaseTag,
				serverMinor: currentInventory.minor,
				consumerRoutes: serverInventory.apiRoutes,
				serverRoutes: currentInventory.apiRoutes,
				webCallsites: serverInventory.webCallsites,
				requestSchemas: serverInventory.requestSchemas,
				responseSchemas: serverInventory.responseSchemas,
			},
		];
		return legacyContracts;
	});

const RELEASE_FIXTURE_RESPONSES: Readonly<
	Record<string, Readonly<Record<string, ReleaseFixtureResponse>>>
> = {
	"v3.22.5": {
		"GET /api/health": {
			contentType: "application/json",
			body: { status: "ok", extra_field_added_by_server: true },
		},
		"GET /api/cluster/info": {
			contentType: "application/json",
			body: {
				cluster_id: "fixture-cluster",
				node_id: "01fixture000000000000000000",
				role: "leader",
				leader_api_base_url: "https://fixture.example",
				term: 7,
				xp_version: "3.22.5",
				extra_field_added_by_server: true,
			},
		},
		"GET /api/admin/nodes": {
			contentType: "application/json",
			body: { items: [], extra_field_added_by_server: true },
		},
		"GET /api/admin/nodes/runtime": {
			contentType: "application/json",
			body: { partial: false, unreachable_nodes: [], items: [] },
		},
		"GET /api/admin/status/events": {
			...statusEventFixture(11),
		},
		"GET /api/version/check": {
			contentType: "application/json",
			body: {
				current: { package: "xp", release_tag: "v3.22.5" },
				latest: { release_tag: "v3.22.5" },
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
			contentType: "application/json",
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
				mihomo_resource_allow_private_targets: false,
				admin_token_present: true,
				admin_token_masked: "****",
			},
		},
		"GET /api/admin/mesh/status": {
			contentType: "application/json",
			body: {
				generated_at: "2026-08-05T00:00:00Z",
				revision: 11,
				local: {},
				peers: [],
				events: [],
			},
		},
		"GET /api/sub/{subscription_token}": {
			contentType: "text/plain",
			body: "proxy-provider-fixture-3.22",
		},
	},
	"v3.21.11": {
		"GET /api/health": {
			contentType: "application/json",
			body: { status: "ok", extra_field_added_by_server: true },
		},
		"GET /api/cluster/info": {
			contentType: "application/json",
			body: {
				cluster_id: "fixture-cluster",
				node_id: "01fixture000000000000000000",
				role: "leader",
				leader_api_base_url: "https://fixture.example",
				term: 7,
				xp_version: "3.21.11",
				extra_field_added_by_server: true,
			},
		},
		"GET /api/admin/nodes": {
			contentType: "application/json",
			body: { items: [], extra_field_added_by_server: true },
		},
		"GET /api/admin/nodes/runtime": {
			contentType: "application/json",
			body: { partial: false, unreachable_nodes: [], items: [] },
		},
		"GET /api/admin/status/events": {
			...statusEventFixture(),
		},
		"GET /api/version/check": {
			contentType: "application/json",
			body: {
				current: { package: "xp", release_tag: "v3.21.11" },
				latest: { release_tag: "v3.21.11" },
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
			contentType: "application/json",
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
		"GET /api/sub/{subscription_token}": {
			contentType: "text/plain",
			body: "proxy-provider-fixture-3.21",
		},
	},
	"v3.20.3": {
		"GET /api/health": {
			contentType: "application/json",
			body: { status: "ok", extra_field_added_by_server: true },
		},
		"GET /api/cluster/info": {
			contentType: "application/json",
			body: {
				cluster_id: "fixture-cluster",
				node_id: "01fixture000000000000000000",
				role: "leader",
				leader_api_base_url: "https://fixture.example",
				term: 7,
				xp_version: "3.20.3",
				extra_field_added_by_server: true,
			},
		},
		"GET /api/admin/nodes": {
			contentType: "application/json",
			body: { items: [], extra_field_added_by_server: true },
		},
		"GET /api/admin/nodes/runtime": {
			contentType: "application/json",
			body: { partial: false, unreachable_nodes: [], items: [] },
		},
		"GET /api/admin/status/events": {
			...statusEventFixture(),
		},
		"GET /api/version/check": {
			contentType: "application/json",
			body: {
				current: { package: "xp", release_tag: "v3.20.3" },
				latest: { release_tag: "v3.20.3" },
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
			contentType: "application/json",
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
		"GET /api/sub/{subscription_token}": {
			contentType: "text/plain",
			body: "proxy-provider-fixture-3.20",
		},
	},
};

function fixtureRequests(
	consumer: ReleaseInventory,
	server: ReleaseInventory,
): ReleaseApiFixture["requests"] {
	return consumer.apiRoutes.map((route) => {
		const separator = route.indexOf(" ");
		const supported = server.apiRoutes.includes(route);
		return {
			route,
			method: route.slice(
				0,
				separator,
			) as ReleaseApiFixture["requests"][number]["method"],
			path: route.slice(separator + 1),
			expectedStatus: !supported
				? 404
				: route === "DELETE /api/admin/nodes/{node_id}"
					? 204
					: 200,
			optional: !supported,
			requestSchema: consumer.requestSchemas[route] ?? [],
		};
	});
}

function fixtureResponses(
	consumer: ReleaseInventory,
	server: ReleaseInventory,
): ReleaseApiFixture["responses"] {
	const serverResponses = RELEASE_FIXTURE_RESPONSES[server.releaseTag] ?? {};
	return Object.entries(consumer.responseSchemas).flatMap(([route, fields]) => {
		if (!server.apiRoutes.includes(route)) return [];
		const response = serverResponses[route];
		if (!response) {
			throw new Error(
				`Missing immutable response fixture for ${server.releaseTag} ${route}`,
			);
		}
		const serverFields = server.responseSchemas[route];
		if (!serverFields) {
			throw new Error(
				`Missing immutable response schema for ${server.releaseTag} ${route}`,
			);
		}
		return [
			{
				route,
				fields: serverFields,
				missingFields: fields.filter((field) => !serverFields.includes(field)),
				...response,
			},
		];
	});
}

export const RELEASE_API_FIXTURES: readonly ReleaseApiFixture[] =
	RELEASE_COMPATIBILITY_CONTRACTS.map((contract) => {
		const consumer = RELEASE_INVENTORIES.find(
			(inventory) => inventory.releaseTag === contract.consumerReleaseTag,
		);
		const server = RELEASE_INVENTORIES.find(
			(inventory) => inventory.releaseTag === contract.serverReleaseTag,
		);
		if (!consumer || !server) {
			throw new Error(
				`Missing immutable inventory for ${contract.serverReleaseTag}`,
			);
		}
		return {
			direction: contract.direction,
			consumerReleaseTag: consumer.releaseTag,
			serverReleaseTag: server.releaseTag,
			consumerSourceCommit: consumer.sourceCommit,
			serverSourceCommit: server.sourceCommit,
			requests: fixtureRequests(consumer, server),
			responses: fixtureResponses(consumer, server),
		};
	});
