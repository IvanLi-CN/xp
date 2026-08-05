import type { AlertsResponse } from "../../src/api/adminAlerts";
import type { AdminEndpointKind } from "../../src/api/adminEndpoints";
import type { NodeHistorySnapshot } from "../../src/api/adminNodeHistory";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminQuotaPolicyNodePolicy } from "../../src/api/adminQuotaPolicyNodePolicy";
import type { AdminRealityDomain } from "../../src/api/adminRealityDomains";
import type { AdminUserAccessItem } from "../../src/api/adminUserAccess";
import type { AdminUserNodeQuota } from "../../src/api/adminUserNodeQuotas";
import type { AdminUserNodeWeightItem } from "../../src/api/adminUserNodeWeights";
import type { AdminUserQuotaSummariesResponse } from "../../src/api/adminUserQuotaSummaries";
import type { AdminUser } from "../../src/api/adminUsers";
import type { ClusterInfoResponse } from "../../src/api/clusterInfo";
import type { HealthResponse } from "../../src/api/health";
import type { ApiCapability } from "../../src/api/releaseInventories";
import type { VersionCheckResponse } from "../../src/api/versionCheck";
import type {
	MockEndpointSeed,
	MockWindowedNodeIpUsage,
	MockWindowedNodeTcpConnections,
	MockWindowedNodeTraffic,
	MockWindowedUserIpUsage,
	MockWindowedUserTraffic,
} from "./apiMock";

export type MockStateSeed = {
	health: HealthResponse;
	clusterInfo: ClusterInfoResponse;
	versionCheck: VersionCheckResponse;
	capabilities: {
		release_tag: string;
		capabilities: ApiCapability[];
		fingerprint: Record<string, string[]>;
	};
	nodes: AdminNode[];
	endpoints: MockEndpointSeed[];
	realityDomains: AdminRealityDomain[];
	users: AdminUser[];
	userAccessByUserId: Record<string, AdminUserAccessItem[]>;
	userAutoAssignEndpointKindsByUserId: Record<string, AdminEndpointKind[]>;
	nodeQuotas: AdminUserNodeQuota[];
	nodeIpUsageByNodeId: Record<string, MockWindowedNodeIpUsage>;
	nodeTcpConnectionsByNodeId: Record<string, MockWindowedNodeTcpConnections>;
	nodeHistoryByNodeId: Record<string, NodeHistorySnapshot>;
	userIpUsageByUserId: Record<string, MockWindowedUserIpUsage>;
	nodeTrafficByNodeId: Record<string, MockWindowedNodeTraffic>;
	userTrafficByUserId: Record<string, MockWindowedUserTraffic>;
	userNodeWeights: Record<string, AdminUserNodeWeightItem[]>;
	userGlobalWeights: Record<string, number>;
	nodeWeightPolicies: Record<string, AdminQuotaPolicyNodePolicy>;
	quotaSummaries?: AdminUserQuotaSummariesResponse;
	alerts: AlertsResponse;
	subscriptions: Record<string, string>;
};

export const DEFAULT_API_CAPABILITIES = {
	release_tag: "v3.22.5",
	capabilities: [
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
	] satisfies ApiCapability[],
	fingerprint: {
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
	},
};
