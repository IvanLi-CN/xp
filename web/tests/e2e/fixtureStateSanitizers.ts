import { fixtureCatalog } from "../../src/fixture-policy/catalog";

export type QuotaResetSource = "user" | "node";

export type UserQuotaReset =
	| { policy: "unlimited"; tz_offset_minutes: number }
	| {
			policy: "monthly";
			day_of_month: number;
			tz_offset_minutes: number;
	  };

export type NodeQuotaReset =
	| { policy: "unlimited"; tz_offset_minutes?: number | null }
	| {
			policy: "monthly";
			day_of_month: number;
			tz_offset_minutes?: number | null;
	  };

export type AdminUser = {
	user_id: string;
	display_name: string;
	subscription_token: string;
	credential_epoch: number;
	priority_tier: "p1" | "p2" | "p3";
	quota_reset: UserQuotaReset;
};

export type AdminNode = {
	node_id: string;
	node_name: string;
	api_base_url: string;
	access_host: string;
	quota_limit_bytes: number;
	quota_reset: NodeQuotaReset;
};

export type AdminEndpoint = {
	endpoint_id: string;
	node_id: string;
	tag: string;
	kind: "vless_reality_vision_tcp" | "ss2022_2022_blake3_aes_128_gcm";
	port: number;
	meta: Record<string, unknown>;
};

export type AdminUserNodeQuota = {
	user_id: string;
	node_id: string;
	quota_limit_bytes: number;
	quota_reset_source: QuotaResetSource;
};

export function normalizeFixtureNode(
	node: AdminNode,
	index: number,
): AdminNode {
	const normalized: AdminNode = {
		node_id: fixtureCatalog.slotString.s32(),
		node_name: fixtureCatalog.slotString.s86(),
		api_base_url: fixtureCatalog.slotString.s87(),
		access_host: fixtureCatalog.slotString.s88(),
		quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
		quota_reset: fixtureCatalog.quota.reset() as NodeQuotaReset,
	};
	if (node.node_id === fixtureCatalog.identifier.nodePrimary()) {
		normalized.node_id = fixtureCatalog.identifier.nodePrimary();
		normalized.node_name = fixtureCatalog.identifier.nodePrimary();
		normalized.api_base_url = fixtureCatalog.url.primaryApi();
		normalized.access_host = fixtureCatalog.host.primary();
	} else if (node.node_id === fixtureCatalog.identifier.nodeSecondary()) {
		normalized.node_id = fixtureCatalog.identifier.nodeSecondary();
		normalized.node_name = fixtureCatalog.identifier.nodeSecondary();
		normalized.api_base_url = fixtureCatalog.url.secondaryApi();
		normalized.access_host = fixtureCatalog.host.secondary();
	} else if (index > 0) {
		normalized.node_id = fixtureCatalog.slotString.s36();
		normalized.node_name = fixtureCatalog.slotString.s37();
		normalized.api_base_url = fixtureCatalog.slotString.s38();
		normalized.access_host = fixtureCatalog.slotString.s39();
	}
	return normalized;
}

export function normalizeFixtureEndpoint(
	endpoint: AdminEndpoint,
	index: number,
): AdminEndpoint {
	const isShadowsocks = endpoint.kind === fixtureCatalog.endpoint.ssKind();
	const normalized: AdminEndpoint = {
		endpoint_id: fixtureCatalog.slotString.s40(),
		node_id: fixtureCatalog.slotString.s32(),
		tag: fixtureCatalog.slotString.s89(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: fixtureCatalog.endpoint.port443(),
		meta: {
			reality: fixtureCatalog.endpoint.reality(),
			reality_keys: fixtureCatalog.endpoint.realityKeys(),
			short_ids: fixtureCatalog.endpoint.shortIds(),
			active_short_id: fixtureCatalog.endpoint.activeShortId(),
			managed_default: true,
		},
	};
	if (endpoint.endpoint_id === fixtureCatalog.identifier.endpointPrimary()) {
		normalized.endpoint_id = fixtureCatalog.identifier.endpointPrimary();
		normalized.node_id = fixtureCatalog.identifier.nodePrimary();
		normalized.tag = fixtureCatalog.identifier.endpointTagPrimary();
		normalized.meta = {
			reality: fixtureCatalog.endpoint.realityAlternate(),
			reality_keys: fixtureCatalog.endpoint.realityKeys(),
			short_ids: fixtureCatalog.endpoint.shortIds(),
			active_short_id: fixtureCatalog.endpoint.activeShortId(),
			managed_default: true,
		};
	} else if (
		endpoint.endpoint_id === fixtureCatalog.identifier.endpointSecondary()
	) {
		normalized.endpoint_id = fixtureCatalog.identifier.endpointSecondary();
		normalized.node_id = fixtureCatalog.identifier.nodeSecondary();
		normalized.tag = fixtureCatalog.identifier.endpointTagSecondary();
	} else if (index > 0) {
		normalized.endpoint_id = fixtureCatalog.slotString.s43();
		normalized.node_id = fixtureCatalog.slotString.s36();
		normalized.tag = fixtureCatalog.slotString.s44();
	}
	if (isShadowsocks) {
		normalized.kind = fixtureCatalog.endpoint.ssKind();
		normalized.port = fixtureCatalog.endpoint.port8443();
		normalized.meta = {
			server_psk_b64: fixtureCatalog.endpoint.serverPskB64(),
		};
	}
	return normalized;
}

export function normalizeFixtureUser(
	user: AdminUser,
	index: number,
): AdminUser {
	const normalized: AdminUser = {
		user_id: fixtureCatalog.identifier.userPrimary(),
		display_name: user.display_name,
		subscription_token: fixtureCatalog.identifier.tokenPrimary(),
		credential_epoch: fixtureCatalog.user.credentialEpoch(),
		priority_tier: fixtureCatalog.user.priorityTierDefault(),
		quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
	};
	if (user.user_id === fixtureCatalog.identifier.userSecondary() || index > 0) {
		normalized.user_id = fixtureCatalog.identifier.userSecondary();
		normalized.subscription_token = fixtureCatalog.identifier.tokenSecondary();
	}
	return normalized;
}

export function normalizeFixtureQuota(
	_quota: AdminUserNodeQuota,
): AdminUserNodeQuota {
	return {
		user_id: fixtureCatalog.identifier.userPrimary(),
		node_id: fixtureCatalog.identifier.nodePrimary(),
		quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
		quota_reset_source: fixtureCatalog.quota.resetSource(),
	};
}
