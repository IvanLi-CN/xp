use super::*;

use std::collections::BTreeMap;

pub(super) const SEED: &str = "seed";

pub(super) fn node(node_id: &str, node_name: &str, access_host: &str) -> Node {
    node_with_api_base(node_id, node_name, access_host, "http://127.0.0.1:0")
}

pub(super) fn node_with_api_base(
    node_id: &str,
    node_name: &str,
    access_host: &str,
    api_base_url: &str,
) -> Node {
    Node {
        node_id: node_id.to_string(),
        node_name: node_name.to_string(),
        access_host: access_host.to_string(),
        api_base_url: api_base_url.to_string(),
        quota_limit_bytes: 0,
        quota_reset: crate::domain::NodeQuotaReset::default(),
    }
}

pub(super) fn user(user_id: &str, display_name: &str) -> User {
    User {
        user_id: user_id.to_string(),
        display_name: display_name.to_string(),
        subscription_token: "token".to_string(),
        credential_epoch: 0,
        priority_tier: Default::default(),
        quota_reset: crate::domain::UserQuotaReset::default(),
    }
}

pub(super) fn endpoint_vless(
    endpoint_id: &str,
    node_id: &str,
    tag: &str,
    port: u16,
    meta: serde_json::Value,
) -> Endpoint {
    Endpoint {
        endpoint_id: endpoint_id.to_string(),
        node_id: node_id.to_string(),
        tag: tag.to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port,
        meta,
    }
}

pub(super) fn endpoint_ss(
    endpoint_id: &str,
    node_id: &str,
    tag: &str,
    port: u16,
    server_psk_b64: &str,
) -> Endpoint {
    Endpoint {
        endpoint_id: endpoint_id.to_string(),
        node_id: node_id.to_string(),
        tag: tag.to_string(),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port,
        meta: serde_json::json!({
            "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
            "server_psk_b64": server_psk_b64,
        }),
    }
}

pub(super) fn vless_meta(
    dest: &str,
    server_names: &[&str],
    managed_default: bool,
) -> serde_json::Value {
    serde_json::json!({
        "reality": {
            "dest": dest,
            "server_names": server_names,
            "fingerprint": "chrome"
        },
        "reality_keys": {
            "private_key": "private",
            "public_key": "public"
        },
        "short_ids": ["0123456789abcdef"],
        "active_short_id": "0123456789abcdef",
        "managed_default": managed_default
    })
}

pub(super) fn membership(
    user_id: &str,
    node_id: &str,
    endpoint_id: &str,
) -> NodeUserEndpointMembership {
    NodeUserEndpointMembership {
        user_id: user_id.to_string(),
        node_id: node_id.to_string(),
        endpoint_id: endpoint_id.to_string(),
    }
}

pub(super) fn egress_probe(
    region: NodeSubscriptionRegion,
    country: &str,
    ip: &str,
) -> NodeEgressProbeState {
    NodeEgressProbeState {
        public_ipv4: Some(ip.to_string()),
        public_ipv6: None,
        selected_public_ip: Some(ip.to_string()),
        geo: crate::inbound_ip_usage::PersistedInboundIpGeo {
            country: country.to_string(),
            region: region.label().to_string(),
            city: String::new(),
            operator: String::new(),
        },
        subscription_region: region,
        checked_at: "2099-01-01T00:00:00Z".to_string(),
        last_success_at: Some("2099-01-01T00:00:00Z".to_string()),
        classification_invalidated_at: None,
        error_summary: None,
    }
}

pub(super) fn probe_map(
    entries: &[(&str, NodeSubscriptionRegion)],
) -> BTreeMap<String, NodeEgressProbeState> {
    entries
        .iter()
        .enumerate()
        .map(|(index, (node_id, region))| {
            let (country, ip) = match region {
                NodeSubscriptionRegion::Japan => ("JP", format!("203.0.113.{}", index + 10)),
                NodeSubscriptionRegion::HongKong => ("HK", format!("203.0.113.{}", index + 20)),
                NodeSubscriptionRegion::Taiwan => ("TW", format!("203.0.113.{}", index + 30)),
                NodeSubscriptionRegion::Korea => ("KR", format!("203.0.113.{}", index + 40)),
                NodeSubscriptionRegion::Singapore => ("SG", format!("203.0.113.{}", index + 50)),
                NodeSubscriptionRegion::Us => ("US", format!("203.0.113.{}", index + 60)),
                NodeSubscriptionRegion::Other => ("DE", format!("203.0.113.{}", index + 70)),
            };
            ((*node_id).to_string(), egress_probe(*region, country, &ip))
        })
        .collect()
}
