import type { AdminEndpoint } from "../../src/api/adminEndpoints";
import type { AdminUserNodeQuotaStatusItem } from "../../src/api/adminUserNodeQuotaStatus";
import type { AdminUserNodeQuota } from "../../src/api/adminUserNodeQuotas";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

export function buildNodeDeletePreviewEndpoint(
	_endpoint: AdminEndpoint,
	index: number,
): Pick<AdminEndpoint, "endpoint_id" | "tag" | "kind" | "port"> {
	const preview = {
		endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
		tag: fixtureCatalog.identifier.endpointTagSecondary(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: fixtureCatalog.endpoint.port443(),
	};
	if (index === 0) {
		preview.endpoint_id = fixtureCatalog.identifier.endpointPrimary();
		preview.tag = fixtureCatalog.identifier.endpointTagPrimary();
	}
	return preview;
}

export function buildUserNodeQuotaStatusItem(
	quota: AdminUserNodeQuota,
	index: number,
): AdminUserNodeQuotaStatusItem {
	const item: AdminUserNodeQuotaStatusItem = {
		user_id:
			quota.user_id === fixtureCatalog.identifier.userPrimary()
				? fixtureCatalog.identifier.userPrimary()
				: fixtureCatalog.identifier.userSecondary(),
		node_id: fixtureCatalog.nodeId.fixture36(),
		quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
		used_bytes: fixtureCatalog.quota.usedBytes(),
		remaining_bytes: fixtureCatalog.quota.remainingBytes(),
		cycle_end_at: fixtureCatalog.timestamp.later(),
		quota_reset_source: fixtureCatalog.quota.resetSource(),
	};
	if (quota.node_id === fixtureCatalog.identifier.nodePrimary()) {
		item.node_id = fixtureCatalog.identifier.nodePrimary();
	} else if (quota.node_id === fixtureCatalog.identifier.nodeSecondary()) {
		item.node_id = fixtureCatalog.identifier.nodeSecondary();
	} else if (index === 0) {
		item.node_id = fixtureCatalog.nodeId.fixture32();
	}
	if (quota.quota_limit_bytes === fixtureCatalog.quota.fiveGiB()) {
		item.quota_limit_bytes = fixtureCatalog.quota.fiveGiB();
		item.remaining_bytes = fixtureCatalog.quota.fiveGiB();
	} else if (quota.quota_limit_bytes === fixtureCatalog.quota.tenGiB()) {
		item.quota_limit_bytes = fixtureCatalog.quota.tenGiB();
		item.remaining_bytes = fixtureCatalog.quota.tenGiB();
	} else if (quota.quota_limit_bytes === fixtureCatalog.quota.oneGiB()) {
		item.quota_limit_bytes = fixtureCatalog.quota.oneGiB();
		item.remaining_bytes = fixtureCatalog.quota.oneGiB();
	}
	if (quota.quota_reset_source === fixtureCatalog.quota.resetSourceNode()) {
		item.quota_reset_source = fixtureCatalog.quota.resetSourceNode();
	}
	return item;
}
