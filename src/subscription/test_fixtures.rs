use super::*;

use std::collections::BTreeMap;

pub(super) const SEED: &str = "seed";

#[derive(Clone, Copy)]
pub(super) enum VlessFixtureMode {
    Standard,
    ManagedDefault,
    ExplicitlyUnmanaged,
}

pub(super) fn node(node_id: &str, node_name: fn() -> &'static str, access_host: &str) -> Node {
    node_with_api_base(
        node_id,
        node_name,
        access_host,
        xp_test_fixtures::subscription_api_loopback(),
    )
}

pub(super) fn node_with_api_base(
    node_id: &str,
    node_name: fn() -> &'static str,
    access_host: &str,
    api_base_url: &str,
) -> Node {
    Node {
        node_id: node_id.to_owned(),
        node_name: node_name().to_owned(),
        access_host: access_host.to_owned(),
        api_base_url: api_base_url.to_owned(),
        quota_limit_bytes: xp_test_fixtures::quota_used_bytes(),
        quota_reset: crate::domain::NodeQuotaReset::default(),
    }
}

pub(super) fn user(display_name: &str) -> User {
    User {
        user_id: xp_test_fixtures::subscription_user_u1().to_owned(),
        display_name: display_name.to_string(),
        subscription_token: xp_test_fixtures::token_fixture460().to_owned(),
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
    mode: VlessFixtureMode,
) -> Endpoint {
    let port = approved_endpoint_port(port);
    let meta = vless_meta(mode);
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

pub(super) fn endpoint_vless_without_server_names(
    endpoint_id: &str,
    node_id: &str,
    tag: &str,
    port: u16,
) -> Endpoint {
    let mut endpoint = endpoint_vless(endpoint_id, node_id, tag, port, VlessFixtureMode::Standard);
    endpoint.meta["reality"]["server_names"] = serde_json::json!([]);
    endpoint
}

pub(super) fn endpoint_ss(
    endpoint_id: &str,
    node_id: &str,
    tag: &str,
    port: u16,
    server_psk_b64: &str,
) -> Endpoint {
    let port = approved_endpoint_port(port);
    match (endpoint_id, node_id, tag) {
        ("e1", "n1", "tag-1") => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_1().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: approved_ss_meta(server_psk_b64),
        },
        ("e1", "n1", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: approved_ss_meta(server_psk_b64),
        },
        ("e2", "n1", "tag-2") => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_2().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: approved_ss_meta(server_psk_b64),
        },
        ("e2", "n1", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: approved_ss_meta(server_psk_b64),
        },
        ("e2", "n2", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: approved_ss_meta(server_psk_b64),
        },
        ("e3", "n2", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e3().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: approved_ss_meta(server_psk_b64),
        },
        ("e3", "n3", _) => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e3().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n3().to_owned(),
            tag: xp_test_fixtures::subscription_tag_ss().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta: approved_ss_meta(server_psk_b64),
        },
        _ => panic!("unknown subscription SS endpoint ({endpoint_id}, {node_id}, {tag})"),
    }
}

fn approved_endpoint_port(port: u16) -> u16 {
    match port {
        443 => xp_test_fixtures::endpoint_port_443(),
        8443 => xp_test_fixtures::endpoint_port_8443(),
        9443 => xp_test_fixtures::endpoint_port_9443(),
        53843 => xp_test_fixtures::endpoint_port_53843(),
        53844 => xp_test_fixtures::endpoint_port_53844(),
        _ => panic!("unapproved subscription fixture port ({port})"),
    }
}

fn vless_meta(mode: VlessFixtureMode) -> serde_json::Value {
    let mut meta = xp_test_fixtures::endpoint_vless_meta().clone();
    match mode {
        VlessFixtureMode::Standard => {}
        VlessFixtureMode::ManagedDefault => meta["managed_default"] = serde_json::json!(true),
        VlessFixtureMode::ExplicitlyUnmanaged => meta["managed_default"] = serde_json::json!(false),
    }
    meta
}

fn approved_ss_meta(server_psk_b64: &str) -> serde_json::Value {
    match server_psk_b64 {
        value if value == xp_test_fixtures::endpoint_server_psk_b64() => {
            xp_test_fixtures::endpoint_ss_meta().clone()
        }
        value if value == xp_test_fixtures::endpoint_server_psk_b64_alternate() => {
            xp_test_fixtures::endpoint_ss_meta_alternate().clone()
        }
        value if value == xp_test_fixtures::endpoint_server_psk_b64_escaped() => {
            xp_test_fixtures::endpoint_ss_meta_escaped().clone()
        }
        _ => panic!("unapproved subscription fixture PSK"),
    }
}

pub(super) fn membership(node_id: &str, endpoint_id: &str) -> NodeUserEndpointMembership {
    match (node_id, endpoint_id) {
        ("n1", "e1") => NodeUserEndpointMembership {
            user_id: xp_test_fixtures::subscription_user_u1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e1().to_owned(),
        },
        ("n1", "e2") => NodeUserEndpointMembership {
            user_id: xp_test_fixtures::subscription_user_u1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
        },
        ("n2", "e2") => NodeUserEndpointMembership {
            user_id: xp_test_fixtures::subscription_user_u1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
        },
        ("n2", "e3") => NodeUserEndpointMembership {
            user_id: xp_test_fixtures::subscription_user_u1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e3().to_owned(),
        },
        ("n2", "e4") => NodeUserEndpointMembership {
            user_id: xp_test_fixtures::subscription_user_u1().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n2().to_owned(),
            endpoint_id: xp_test_fixtures::subscription_endpoint_e4().to_owned(),
        },
        ("n3", "e3") => NodeUserEndpointMembership {
            user_id: xp_test_fixtures::subscription_user_u1().to_owned(),
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
        public_ipv4: Some(xp_test_fixtures::address_documentation203_0_113_30().to_owned()),
        public_ipv6: None,
        selected_public_ip: Some(xp_test_fixtures::address_documentation203_0_113_30().to_owned()),
        geo: crate::inbound_ip_usage::PersistedInboundIpGeo {
            country: country.to_string(),
            region: region.label().to_string(),
            city: String::new(),
            operator: String::new(),
        },
        subscription_region: region,
        checked_at: xp_test_fixtures::timestamp_at20990101_t000000_z().to_owned(),
        last_success_at: Some(xp_test_fixtures::timestamp_at20990101_t000000_z().to_owned()),
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
