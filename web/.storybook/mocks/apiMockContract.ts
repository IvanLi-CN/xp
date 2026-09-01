import type { AlertsResponse } from "../../src/api/adminAlerts";
import type { AdminEndpointKind } from "../../src/api/adminEndpoints";
import type { NodeHistorySnapshot } from "../../src/api/adminNodeHistory";
import type {
	AdminNode,
	AdminNodeMihomoResourcePolicy,
} from "../../src/api/adminNodes";
import type { AdminQuotaPolicyNodePolicy } from "../../src/api/adminQuotaPolicyNodePolicy";
import type { AdminRealityDomain } from "../../src/api/adminRealityDomains";
import type { ServiceMonitorSummary } from "../../src/api/adminServiceMonitors";
import type { AdminUserAccessItem } from "../../src/api/adminUserAccess";
import type { AdminUserNodeQuota } from "../../src/api/adminUserNodeQuotas";
import type { AdminUserNodeWeightItem } from "../../src/api/adminUserNodeWeights";
import type { AdminUserQuotaSummariesResponse } from "../../src/api/adminUserQuotaSummaries";
import type { AdminUser } from "../../src/api/adminUsers";
import type { ClusterInfoResponse } from "../../src/api/clusterInfo";
import type { HealthResponse } from "../../src/api/health";
import {
	API_CAPABILITIES,
	type ApiCapability,
	CURRENT_API_FINGERPRINT,
} from "../../src/api/releaseInventories";
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
	nodeMihomoResourcePolicyByNodeId: Record<
		string,
		AdminNodeMihomoResourcePolicy
	>;
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
	serviceMonitors?: ServiceMonitorSummary[];
};

export const DEFAULT_API_CAPABILITIES = {
	release_tag: "v3.22.5",
	capabilities: [
		...API_CAPABILITIES,
		"admin.endpoint-mihomo-smux",
		"admin.history-repositories",
		"admin.repository-history",
		"admin.service-monitors",
	] satisfies ApiCapability[],
	fingerprint: Object.fromEntries(
		Object.entries(CURRENT_API_FINGERPRINT).map(([path, fields]) => [
			path,
			[...fields],
		]),
	),
};
