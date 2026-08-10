import type { AdminEndpoint } from "../../src/api/adminEndpoints";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminUserNodeQuota } from "../../src/api/adminUserNodeQuotas";
import type { AdminUser } from "../../src/api/adminUsers";
import type { NodeQuotaReset, UserQuotaReset } from "../../src/api/quotaReset";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

type FixtureEndpoint = AdminEndpoint & {
	active_short_id?: string;
	short_ids?: string[];
};

export function sanitizeFixtureNode(
	_node: AdminNode,
	index: number,
): AdminNode {
	const egressProbe = _node.egress_probe
		? {
				public_ipv4: fixtureCatalog.address.tertiaryIpv4(),
				public_ipv6: fixtureCatalog.optional.none(),
				selected_public_ip: fixtureCatalog.address.tertiaryIpv4(),
				country_code: fixtureCatalog.string.none(),
				geo_region: fixtureCatalog.string.none(),
				geo_city: fixtureCatalog.string.none(),
				geo_operator: fixtureCatalog.string.none(),
				subscription_region: "japan" as const,
				checked_at: fixtureCatalog.timestamp.recent(),
				last_success_at: fixtureCatalog.timestamp.recent(),
				stale: false,
				error_summary: fixtureCatalog.string.none(),
			}
		: undefined;
	const normalized: AdminNode = {
		node_id: fixtureCatalog.slotString.s32(),
		node_name: fixtureCatalog.slotString.s33(),
		access_host: fixtureCatalog.slotString.s35(),
		api_base_url: fixtureCatalog.slotString.s34(),
		quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
		quota_reset: fixtureCatalog.quota.reset() as NodeQuotaReset,
		...(egressProbe ? { egress_probe: egressProbe } : {}),
	};
	if (index > 0) {
		normalized.node_id = fixtureCatalog.slotString.s36();
		normalized.node_name = fixtureCatalog.slotString.s37();
		normalized.access_host = fixtureCatalog.slotString.s39();
		normalized.api_base_url = fixtureCatalog.slotString.s38();
	}
	if (_node.node_id === fixtureCatalog.slotString.s63()) {
		normalized.node_id = fixtureCatalog.slotString.s63();
		normalized.node_name = fixtureCatalog.slotString.s63();
		normalized.access_host = fixtureCatalog.slotString.s114();
		normalized.api_base_url = fixtureCatalog.slotString.s115();
	}
	return normalized;
}

export function sanitizeFixtureEndpoint(
	_endpoint: FixtureEndpoint,
	index: number,
): FixtureEndpoint {
	const normalized: FixtureEndpoint = {
		endpoint_id: fixtureCatalog.slotString.s40(),
		node_id: fixtureCatalog.slotString.s32(),
		tag: fixtureCatalog.slotString.s41(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: fixtureCatalog.endpoint.port443(),
		meta: {
			reality: fixtureCatalog.endpoint.reality(),
			reality_keys: fixtureCatalog.endpoint.realityKeys(),
			short_ids: fixtureCatalog.endpoint.shortIds(),
			active_short_id: fixtureCatalog.endpoint.activeShortId(),
		},
		short_ids: fixtureCatalog.endpoint.shortIds(),
		active_short_id: fixtureCatalog.endpoint.activeShortId(),
	};
	if (index > 0) {
		normalized.kind = fixtureCatalog.endpoint.ssKind();
		normalized.endpoint_id = fixtureCatalog.slotString.s43();
		normalized.node_id = fixtureCatalog.slotString.s36();
		normalized.tag = fixtureCatalog.slotString.s44();
		normalized.port = fixtureCatalog.endpoint.port8443();
		normalized.meta = {
			server_psk_b64: fixtureCatalog.endpoint.serverPskB64(),
		};
	}
	return normalized;
}

export function sanitizeFixtureUser(
	_user: AdminUser,
	index: number,
): AdminUser {
	const normalized: AdminUser = {
		user_id: fixtureCatalog.identifier.userPrimary(),
		display_name: _user.display_name,
		subscription_token: fixtureCatalog.slotString.s45(),
		credential_epoch: fixtureCatalog.user.credentialEpoch(),
		priority_tier: fixtureCatalog.user.priorityTierDefault(),
		quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
	};
	if (index > 0) {
		normalized.user_id = fixtureCatalog.identifier.userSecondary();
		normalized.subscription_token = fixtureCatalog.slotString.s46();
	}
	return normalized;
}

export function sanitizeFixtureQuota(
	quota: AdminUserNodeQuota,
): AdminUserNodeQuota {
	const isSecondary =
		quota.user_id === fixtureCatalog.identifier.userSecondary() ||
		quota.node_id === fixtureCatalog.identifier.nodeSecondary();
	return {
		user_id: isSecondary
			? fixtureCatalog.identifier.userSecondary()
			: fixtureCatalog.identifier.userPrimary(),
		node_id: isSecondary
			? fixtureCatalog.identifier.nodeSecondary()
			: fixtureCatalog.identifier.nodePrimary(),
		quota_limit_bytes:
			quota.quota_limit_bytes === fixtureCatalog.quota.fiveGiB()
				? fixtureCatalog.quota.fiveGiB()
				: fixtureCatalog.quota.limitBytes(),
		quota_reset_source: fixtureCatalog.quota.resetSource(),
	};
}
