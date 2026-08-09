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

export function sanitizeFixtureNode(node: AdminNode, index: number): AdminNode {
	const egressProbe = node.egress_probe
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
	return normalized;
}

export function sanitizeFixtureEndpoint(
	endpoint: FixtureEndpoint,
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
		normalized.endpoint_id = fixtureCatalog.slotString.s43();
		normalized.node_id = fixtureCatalog.slotString.s36();
		normalized.tag = fixtureCatalog.slotString.s44();
	}
	if (endpoint.kind === fixtureCatalog.endpoint.ssKind()) {
		normalized.kind = fixtureCatalog.endpoint.ssKind();
		normalized.port = fixtureCatalog.endpoint.port8443();
		normalized.meta = {
			server_psk_b64: fixtureCatalog.endpoint.serverPskB64(),
		};
	}
	return normalized;
}

export function sanitizeFixtureUser(user: AdminUser, index: number): AdminUser {
	const normalized: AdminUser = {
		user_id: fixtureCatalog.identifier.userPrimary(),
		display_name: user.display_name,
		subscription_token: fixtureCatalog.slotString.s45(),
		credential_epoch: user.credential_epoch,
		priority_tier: user.priority_tier,
		quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
	};
	if (index > 0) {
		normalized.user_id = fixtureCatalog.identifier.userSecondary();
		normalized.subscription_token = fixtureCatalog.slotString.s46();
	}
	return normalized;
}

export function sanitizeFixtureQuota(
	_quota: AdminUserNodeQuota,
): AdminUserNodeQuota {
	return {
		user_id: fixtureCatalog.identifier.userPrimary(),
		node_id: fixtureCatalog.identifier.nodePrimary(),
		quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
		quota_reset_source: fixtureCatalog.quota.resetSource(),
	};
}
