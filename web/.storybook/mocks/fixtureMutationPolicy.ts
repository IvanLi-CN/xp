import type {
	AdminEndpoint,
	AdminEndpointCreateRequest,
	AdminEndpointPatchRequest,
} from "../../src/api/adminEndpoints";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminRealityDomain } from "../../src/api/adminRealityDomains";
import type { AdminUserNodeWeightItem } from "../../src/api/adminUserNodeWeights";
import type {
	AdminUser,
	AdminUserPatchRequest,
} from "../../src/api/adminUsers";
import type { NodeQuotaReset, UserQuotaReset } from "../../src/api/quotaReset";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

type NodePatchPayload = {
	node_name?: string;
	access_host?: string;
	api_base_url?: string;
	quota_limit_bytes?: number;
	quota_reset?: NodeQuotaReset;
};

type RealityDomainPatchPayload = {
	server_name?: string;
	disabled_node_ids?: string[];
};

export function buildFixtureEndpointIdentifierResponse(
	endpoint: Pick<AdminEndpoint, "endpoint_id">,
) {
	return { endpoint_id: endpoint.endpoint_id };
}

export function buildFixtureUserNodeWeightItem(
	node: Pick<AdminNode, "node_id">,
	weight: number,
): AdminUserNodeWeightItem {
	return {
		node_id: node.node_id,
		weight,
	};
}

export function buildFixtureSubscriptionText(format: string | null): string {
	if (format === "clash") return fixtureCatalog.subscription.clash();
	if (format === "mihomo_provider") return fixtureCatalog.subscription.rawUri();
	if (format === "mihomo_provider_system") {
		return fixtureCatalog.subscription.clash();
	}
	if (format === "mihomo") return fixtureCatalog.subscription.clash();
	return fixtureCatalog.subscription.rawUri();
}

export function isFixtureNodeId(value: string): boolean {
	return (
		value === fixtureCatalog.nodeId.fixture32() ||
		value === fixtureCatalog.nodeId.fixture36() ||
		value === fixtureCatalog.nodeId.fixture63()
	);
}

export function isFixtureRealityServerName(value: string): boolean {
	return (
		value === fixtureCatalog.host.serverPrimary() ||
		value === fixtureCatalog.host.serverSecondary() ||
		value === fixtureCatalog.host.tertiary()
	);
}

export function buildFixtureDisabledNodeIds(nodeIds: string[]): string[] {
	return [
		...(nodeIds.includes(fixtureCatalog.nodeId.fixture63())
			? [fixtureCatalog.nodeId.fixture63()]
			: []),
		...(nodeIds.includes(fixtureCatalog.nodeId.fixture36())
			? [fixtureCatalog.nodeId.fixture36()]
			: []),
		...(nodeIds.includes(fixtureCatalog.nodeId.fixture32())
			? [fixtureCatalog.nodeId.fixture32()]
			: []),
	];
}

export function normalizeFixtureNodePatch(
	node: AdminNode,
	payload: NodePatchPayload,
): AdminNode | undefined {
	if (
		(payload.node_name !== undefined && payload.node_name !== node.node_name) ||
		(payload.access_host !== undefined &&
			payload.access_host !== node.access_host) ||
		(payload.api_base_url !== undefined &&
			payload.api_base_url !== node.api_base_url)
	) {
		return undefined;
	}

	let updated = node;
	if (payload.quota_limit_bytes !== undefined) {
		if (payload.quota_limit_bytes === fixtureCatalog.quota.usedBytes()) {
			updated = {
				...updated,
				quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
			};
		} else if (
			payload.quota_limit_bytes === fixtureCatalog.quota.limitBytes()
		) {
			updated = {
				...updated,
				quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
			};
		} else if (payload.quota_limit_bytes === fixtureCatalog.quota.fiveGiB()) {
			updated = {
				...updated,
				quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
			};
		} else {
			return undefined;
		}
	}
	if (payload.quota_reset !== undefined) {
		if (
			JSON.stringify(payload.quota_reset) !==
			JSON.stringify(fixtureCatalog.quota.reset())
		) {
			return undefined;
		}
		updated = {
			...updated,
			quota_reset: fixtureCatalog.quota.reset() as NodeQuotaReset,
		};
	}
	return updated;
}

export function normalizeFixtureUserPatch(
	user: AdminUser,
	payload: AdminUserPatchRequest,
): AdminUser | undefined {
	let updated = user;
	if (payload.priority_tier !== undefined) {
		if (payload.priority_tier === fixtureCatalog.user.priorityTierPrimary()) {
			updated = {
				...updated,
				priority_tier: fixtureCatalog.user.priorityTierPrimary(),
			};
		} else if (
			payload.priority_tier === fixtureCatalog.user.priorityTierCreated()
		) {
			updated = {
				...updated,
				priority_tier: fixtureCatalog.user.priorityTierCreated(),
			};
		} else if (
			payload.priority_tier === fixtureCatalog.user.priorityTierDefault()
		) {
			updated = {
				...updated,
				priority_tier: fixtureCatalog.user.priorityTierDefault(),
			};
		} else {
			return undefined;
		}
	}
	if (payload.quota_reset !== undefined) {
		if (
			JSON.stringify(payload.quota_reset) ===
			JSON.stringify(fixtureCatalog.quota.reset())
		) {
			updated = {
				...updated,
				quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
			};
		} else if (
			JSON.stringify(payload.quota_reset) ===
			JSON.stringify(fixtureCatalog.quota.resetUserMidMonth())
		) {
			updated = {
				...updated,
				quota_reset: fixtureCatalog.quota.resetUserMidMonth() as UserQuotaReset,
			};
		} else if (
			JSON.stringify(payload.quota_reset) ===
			JSON.stringify(fixtureCatalog.quota.resetUserUnlimited())
		) {
			updated = {
				...updated,
				quota_reset:
					fixtureCatalog.quota.resetUserUnlimited() as UserQuotaReset,
			};
		} else {
			return undefined;
		}
	}
	return updated;
}

export function normalizeFixtureRealityDomainPatch(
	existing: AdminRealityDomain,
	payload: RealityDomainPatchPayload,
): AdminRealityDomain | undefined {
	let updated = existing;
	if (payload.server_name !== undefined) {
		if (!isFixtureRealityServerName(payload.server_name)) return undefined;
		updated = {
			...updated,
			server_name:
				payload.server_name === fixtureCatalog.host.serverPrimary()
					? fixtureCatalog.host.serverPrimary()
					: payload.server_name === fixtureCatalog.host.serverSecondary()
						? fixtureCatalog.host.serverSecondary()
						: fixtureCatalog.host.tertiary(),
		};
	}
	if (payload.disabled_node_ids !== undefined) {
		if (!payload.disabled_node_ids.every(isFixtureNodeId)) return undefined;
		updated = {
			...updated,
			disabled_node_ids: buildFixtureDisabledNodeIds(payload.disabled_node_ids),
		};
	}
	return updated;
}

export function isFixtureEndpointCreateRequest(
	payload: AdminEndpointCreateRequest,
	nodes: AdminNode[],
): boolean {
	return (
		isFixtureNodeId(payload.node_id) &&
		nodes.some((node) => node.node_id === payload.node_id) &&
		(payload.kind === fixtureCatalog.endpoint.vlessKind() ||
			payload.kind === fixtureCatalog.endpoint.ssKind()) &&
		(payload.port === fixtureCatalog.endpoint.port443() ||
			payload.port === fixtureCatalog.endpoint.port8443() ||
			payload.port === fixtureCatalog.endpoint.port9443())
	);
}

export function normalizeFixtureEndpointPatch<T extends AdminEndpoint>(
	endpoint: T,
	payload: AdminEndpointPatchRequest,
): T | undefined {
	if (
		payload.canary_upstream !== undefined ||
		payload.accepted_authorities !== undefined ||
		payload.mihomo_smux !== undefined
	) {
		return undefined;
	}

	let updated = endpoint;
	if (payload.port !== undefined) {
		if (
			payload.port !== fixtureCatalog.endpoint.port443() &&
			payload.port !== fixtureCatalog.endpoint.port8443() &&
			payload.port !== fixtureCatalog.endpoint.port9443()
		) {
			return undefined;
		}
		updated = {
			...updated,
			port:
				payload.port === fixtureCatalog.endpoint.port8443()
					? fixtureCatalog.endpoint.port8443()
					: payload.port === fixtureCatalog.endpoint.port9443()
						? fixtureCatalog.endpoint.port9443()
						: fixtureCatalog.endpoint.port443(),
		};
	}
	if (payload.reality !== undefined) {
		updated = {
			...updated,
			meta: {
				...updated.meta,
				reality: fixtureCatalog.endpoint.reality(),
			},
		};
	}
	return updated;
}
