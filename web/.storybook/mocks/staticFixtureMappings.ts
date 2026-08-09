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
	const item = {
		user_id:
			quota.user_id === fixtureCatalog.identifier.userPrimary()
				? fixtureCatalog.identifier.userPrimary()
				: fixtureCatalog.identifier.userSecondary(),
		node_id: fixtureCatalog.slotString.s36(),
		quota_limit_bytes:
			quota.quota_limit_bytes === fixtureCatalog.slotNumber.n16()
				? fixtureCatalog.slotNumber.n16()
				: fixtureCatalog.quota.limitBytes(),
		used_bytes: fixtureCatalog.quota.usedBytes(),
		remaining_bytes:
			quota.quota_limit_bytes === fixtureCatalog.slotNumber.n16()
				? fixtureCatalog.slotNumber.n16()
				: fixtureCatalog.quota.remainingBytes(),
		cycle_end_at: fixtureCatalog.timestamp.later(),
		quota_reset_source: fixtureCatalog.quota.resetSource(),
	};
	if (quota.node_id === fixtureCatalog.identifier.nodePrimary()) {
		item.node_id = fixtureCatalog.identifier.nodePrimary();
	} else if (quota.node_id === fixtureCatalog.identifier.nodeSecondary()) {
		item.node_id = fixtureCatalog.identifier.nodeSecondary();
	} else if (index === 0) {
		item.node_id = fixtureCatalog.slotString.s32();
	}
	return item;
}
