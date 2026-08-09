use super::*;

use std::collections::BTreeMap;

pub(super) const SEED: &str = "seed";

pub(super) fn node(node_id: &str, node_name: fn() -> &'static str, access_host: &str) -> Node {
    node_with_api_base(node_id, node_name, access_host, "http://127.0.0.1:0")
}

pub(super) fn node_with_api_base(
    node_id: &str,
    node_name: fn() -> &'static str,
    access_host: &str,
    api_base_url: &str,
) -> Node {
    match (node_id, access_host, api_base_url) {
        ("n1", "example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_example().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "", "http://127.0.0.1:0") | ("n1", "   ", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_empty().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "tokyo-a.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_tokyo_a().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "singapore-a.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_singapore().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "relay.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_relay().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "jp.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_jp().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "relay-a.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_relay_a().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "new-host.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_new().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "us.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_us().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "alpha.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_alpha().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_example().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "hkl.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_hkl().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "relay-b.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_relay_b().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "relay-jp.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_relay_jp().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "beta.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_beta().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n3", "mystery.example.com", "http://127.0.0.1:0") => Node {
            node_id: xp_test_fixtures::subscription_node_n3().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_mystery().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "shared.example.com", "https://tokyo-a.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_shared().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_tokyo_a().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "shared.example.com", "https://tokyo-b.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_shared().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_tokyo_b().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "shared.example.com", "https://tokyo-b.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_shared().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_tokyo_b().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n3", "seoul.example.com", "https://seoul-a.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n3().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_seoul().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_seoul_a().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "relay.example.com", "https://127.0.0.1:62416") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_relay().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_loopback_https().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "shared.example.com", "https://shared-api.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_shared().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_shared().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "shared.example.com", "https://shared-api.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_shared().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_shared().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "endpoint-node.example.com", "https://xp-node.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_endpoint_node().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_xp_node().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "shared.example.com", "https://aardvark.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_shared().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_aardvark().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "a.b.example.com", "https://dot.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_dot().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_dot().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "a-b.example.com", "https://dash.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_dash().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_dash().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n1", "shared.example.com", "https://unsubscribed.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_shared().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_unsubscribed().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        ("n2", "shared.example.com", "https://subscribed.example.com") => Node {
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            node_name: node_name().to_owned(),
            access_host: xp_test_fixtures::subscription_host_shared().to_owned(),
            api_base_url: xp_test_fixtures::subscription_api_subscribed().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: crate::domain::NodeQuotaReset::default(),
        },
        _ => panic!("unknown subscription node fixture ({node_id}, {access_host}, {api_base_url})"),
    }
}

pub(super) fn user(user_id: &str, display_name: &str) -> User {
    User {
        user_id: user_id.to_string(),
        display_name: display_name.to_string(),
        subscription_token: xp_test_fixtures::slot_s460().to_owned(),
        credential_epoch: 0,
        priority_tier: Default::default(),
        quota_reset: crate::domain::UserQuotaReset::default(),
    }
}

pub(super) fn endpoint_vless(
    endpoint_id: &str,
    node_id: &str,
    _tag: &str,
    port: u16,
    meta: serde_json::Value,
) -> Endpoint {
    match (endpoint_id, node_id) {
        ("e1", "n1") => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_vless().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port,
            meta,
        },
        ("e2", "n1") => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_vless().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port,
            meta,
        },
        ("e4", "n2") => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e4().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            tag: xp_test_fixtures::subscription_tag_vless().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port,
            meta,
        },
        _ => panic!("unknown subscription VLESS endpoint ({endpoint_id}, {node_id})"),
    }
}

pub(super) fn endpoint_ss(
    endpoint_id: &str,
    node_id: &str,
    tag: &str,
    port: u16,
    server_psk_b64: &str,
) -> Endpoint {
    match (endpoint_id, node_id, tag) {
        ("e1", "n1", "tag-1") => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_1().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({
                "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                "server_psk_b64": server_psk_b64,
            }),
        },
        ("e1", "n1", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({
                "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                "server_psk_b64": server_psk_b64,
            }),
        },
        ("e2", "n1", "tag-2") => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_2().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({
                "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                "server_psk_b64": server_psk_b64,
            }),
        },
        ("e2", "n1", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({
                "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                "server_psk_b64": server_psk_b64,
            }),
        },
        ("e2", "n2", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({
                "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                "server_psk_b64": server_psk_b64,
            }),
        },
        ("e3", "n2", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e3().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({
                "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                "server_psk_b64": server_psk_b64,
            }),
        },
        ("e3", "n3", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e3().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n3().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: serde_json::json!({
                "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
                "server_psk_b64": server_psk_b64,
            }),
        },
        _ => panic!("unknown subscription SS endpoint ({endpoint_id}, {node_id}, {tag})"),
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
    match (node_id, endpoint_id) {
        ("n1", "e1") => NodeUserEndpointMembership {
            user_id: user_id.to_string(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e1().to_owned(),
        },
        ("n1", "e2") => NodeUserEndpointMembership {
            user_id: user_id.to_string(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
        },
        ("n2", "e2") => NodeUserEndpointMembership {
            user_id: user_id.to_string(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
        },
        ("n2", "e3") => NodeUserEndpointMembership {
            user_id: user_id.to_string(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e3().to_owned(),
        },
        ("n2", "e4") => NodeUserEndpointMembership {
            user_id: user_id.to_string(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e4().to_owned(),
        },
        ("n3", "e3") => NodeUserEndpointMembership {
            user_id: user_id.to_string(),
            node_id: xp_test_fixtures::subscription_node_n3().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e3().to_owned(),
        },
        _ => panic!("unknown subscription membership ({node_id}, {endpoint_id})"),
    }
}

pub(super) fn egress_probe(
    region: NodeSubscriptionRegion,
    country: &str,
    _ip: &str,
) -> NodeEgressProbeState {
    NodeEgressProbeState {
        public_ipv4: Some(xp_test_fixtures::slot_s463().to_owned()),
        public_ipv6: None,
        selected_public_ip: Some(xp_test_fixtures::slot_s463().to_owned()),
        geo: crate::inbound_ip_usage::PersistedInboundIpGeo {
            country: country.to_string(),
            region: region.label().to_string(),
            city: String::new(),
            operator: String::new(),
        },
        subscription_region: region,
        checked_at: xp_test_fixtures::slot_s464().to_owned(),
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
