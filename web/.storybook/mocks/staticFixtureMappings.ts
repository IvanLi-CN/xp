import type { AdminEndpoint } from "../../src/api/adminEndpoints";
import type { AdminUserNodeQuotaStatusItem } from "../../src/api/adminUserNodeQuotaStatus";
import type { AdminUserNodeQuota } from "../../src/api/adminUserNodeQuotas";
import type { AdminUserQuotaSummaryItem } from "../../src/api/adminUserQuotaSummaries";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

export function buildNodeDeletePreviewEndpoint(
	endpoint: AdminEndpoint,
): Pick<AdminEndpoint, "endpoint_id" | "tag" | "kind" | "port"> {
	return {
		endpoint_id: endpoint.endpoint_id,
		tag: endpoint.tag,
		kind: endpoint.kind,
		port: endpoint.port,
	};
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
	} else if (quota.quota_limit_bytes === fixtureCatalog.quota.usedBytes()) {
		item.quota_limit_bytes = fixtureCatalog.quota.usedBytes();
		item.remaining_bytes = fixtureCatalog.quota.usedBytes();
		item.cycle_end_at = fixtureCatalog.optional.none();
	}
	if (quota.quota_reset_source === fixtureCatalog.quota.resetSourceNode()) {
		item.quota_reset_source = fixtureCatalog.quota.resetSourceNode();
	}
	return item;
}

function buildFixtureQuotaSummaryIdentity(userId: string): string {
	return userId === fixtureCatalog.identifier.userPrimary()
		? fixtureCatalog.identifier.userPrimary()
		: fixtureCatalog.identifier.userSecondary();
}

export function buildFixtureUserQuotaSummaryItem(
	userId: string,
	quotas: readonly AdminUserNodeQuota[],
): AdminUserQuotaSummaryItem {
	const unlimited = quotas.some(
		(quota) => quota.quota_limit_bytes === fixtureCatalog.quota.usedBytes(),
	);
	const total = unlimited
		? fixtureCatalog.quota.usedBytes()
		: quotas.reduce(
				(sum, quota) => sum + quota.quota_limit_bytes,
				fixtureCatalog.quota.usedBytes(),
			);
	const identity = buildFixtureQuotaSummaryIdentity(userId);

	if (unlimited) {
		return {
			user_id: identity,
			quota_limit_kind: "unlimited",
			quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
			used_bytes: fixtureCatalog.quota.usedBytes(),
			remaining_bytes: fixtureCatalog.quota.usedBytes(),
		};
	}
	if (total === fixtureCatalog.quota.oneGiB()) {
		return {
			user_id: identity,
			quota_limit_kind: "fixed",
			quota_limit_bytes: fixtureCatalog.quota.oneGiB(),
			used_bytes: fixtureCatalog.quota.usedBytes(),
			remaining_bytes: fixtureCatalog.quota.oneGiB(),
		};
	}
	if (total === fixtureCatalog.quota.fourGiB()) {
		return {
			user_id: identity,
			quota_limit_kind: "fixed",
			quota_limit_bytes: fixtureCatalog.quota.fourGiB(),
			used_bytes: fixtureCatalog.quota.usedBytes(),
			remaining_bytes: fixtureCatalog.quota.fourGiB(),
		};
	}
	if (total === fixtureCatalog.quota.fiveGiB()) {
		return {
			user_id: identity,
			quota_limit_kind: "fixed",
			quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
			used_bytes: fixtureCatalog.quota.usedBytes(),
			remaining_bytes: fixtureCatalog.quota.fiveGiB(),
		};
	}
	if (total === fixtureCatalog.quota.tenGiB()) {
		return {
			user_id: identity,
			quota_limit_kind: "fixed",
			quota_limit_bytes: fixtureCatalog.quota.tenGiB(),
			used_bytes: fixtureCatalog.quota.usedBytes(),
			remaining_bytes: fixtureCatalog.quota.tenGiB(),
		};
	}
	if (total === fixtureCatalog.quota.elevenGiB()) {
		return {
			user_id: identity,
			quota_limit_kind: "fixed",
			quota_limit_bytes: fixtureCatalog.quota.elevenGiB(),
			used_bytes: fixtureCatalog.quota.usedBytes(),
			remaining_bytes: fixtureCatalog.quota.elevenGiB(),
		};
	}
	if (total === fixtureCatalog.quota.fifteenGiB()) {
		return {
			user_id: identity,
			quota_limit_kind: "fixed",
			quota_limit_bytes: fixtureCatalog.quota.fifteenGiB(),
			used_bytes: fixtureCatalog.quota.usedBytes(),
			remaining_bytes: fixtureCatalog.quota.fifteenGiB(),
		};
	}
	return {
		user_id: identity,
		quota_limit_kind: "mixed",
		quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
		used_bytes: fixtureCatalog.quota.usedBytes(),
		remaining_bytes: fixtureCatalog.quota.usedBytes(),
	};
}
