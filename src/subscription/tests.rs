use super::*;

use pretty_assertions::assert_eq;
use serde_yaml::Value;
use std::collections::BTreeMap;

const SEED: &str = "seed";

fn node(node_id: &str, node_name: &str, access_host: &str) -> Node {
    node_with_api_base(node_id, node_name, access_host, "http://127.0.0.1:0")
}

fn node_with_api_base(
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

fn user(user_id: &str, display_name: &str) -> User {
    User {
        user_id: user_id.to_string(),
        display_name: display_name.to_string(),
        subscription_token: "token".to_string(),
        credential_epoch: 0,
        priority_tier: Default::default(),
        quota_reset: crate::domain::UserQuotaReset::default(),
    }
}

fn endpoint_vless(
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

fn endpoint_ss(
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

fn vless_meta(dest: &str, server_names: &[&str], managed_default: bool) -> serde_json::Value {
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

fn membership(user_id: &str, node_id: &str, endpoint_id: &str) -> NodeUserEndpointMembership {
    NodeUserEndpointMembership {
        user_id: user_id.to_string(),
        node_id: node_id.to_string(),
        endpoint_id: endpoint_id.to_string(),
    }
}

fn egress_probe(region: NodeSubscriptionRegion, country: &str, ip: &str) -> NodeEgressProbeState {
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

fn probe_map(entries: &[(&str, NodeSubscriptionRegion)]) -> BTreeMap<String, NodeEgressProbeState> {
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

#[test]
fn build_mihomo_base_region_map_falls_back_to_legacy_slug_before_first_successful_probe() {
    let nodes = vec![
        node("n1", "tokyo-a", "tokyo-a.example.com"),
        node("n2", "hkl", "hkl.example.com"),
        node("n3", "mystery", "mystery.example.com"),
    ];
    let mut probes = BTreeMap::new();
    probes.insert(
        "n1".to_string(),
        NodeEgressProbeState {
            checked_at: "2026-04-24T00:00:00Z".to_string(),
            ..NodeEgressProbeState::default()
        },
    );

    let region_map = build_mihomo_base_region_map(&nodes, &probes);

    assert_eq!(
        region_map.get("tokyo-a"),
        Some(&NodeSubscriptionRegion::Japan)
    );
    assert_eq!(
        region_map.get("hkl"),
        Some(&NodeSubscriptionRegion::HongKong)
    );
    assert_eq!(
        region_map.get("mystery"),
        Some(&NodeSubscriptionRegion::Other)
    );
}

#[test]
fn build_mihomo_base_region_map_keeps_singapore_slug_as_other_before_first_probe() {
    let nodes = vec![node("n1", "singapore-a", "singapore-a.example.com")];

    let region_map = build_mihomo_base_region_map(&nodes, &BTreeMap::new());

    assert_eq!(
        region_map.get("singapore-a"),
        Some(&NodeSubscriptionRegion::Other)
    );
}

#[test]
fn build_mihomo_base_region_map_prefers_successful_probe_over_legacy_slug() {
    let nodes = vec![node("n1", "tokyo-a", "tokyo-a.example.com")];
    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Taiwan)]);

    let region_map = build_mihomo_base_region_map(&nodes, &probes);

    assert_eq!(
        region_map.get("tokyo-a"),
        Some(&NodeSubscriptionRegion::Taiwan)
    );
}

#[test]
fn build_mihomo_base_region_map_falls_back_to_legacy_slug_after_failed_first_probe() {
    let nodes = vec![node("n1", "tokyo-a", "tokyo-a.example.com")];
    let mut probes = BTreeMap::new();
    probes.insert(
        "n1".to_string(),
        NodeEgressProbeState {
            checked_at: "2026-04-24T01:00:00Z".to_string(),
            selected_public_ip: Some("198.51.100.9".to_string()),
            subscription_region: NodeSubscriptionRegion::Other,
            error_summary: Some("country.is lookup failed".to_string()),
            ..NodeEgressProbeState::default()
        },
    );

    let region_map = build_mihomo_base_region_map(&nodes, &probes);

    assert_eq!(
        region_map.get("tokyo-a"),
        Some(&NodeSubscriptionRegion::Japan)
    );
}

#[test]
fn build_mihomo_base_region_map_keeps_last_successful_probe_region_when_stale() {
    let nodes = vec![node("n1", "tokyo-a", "tokyo-a.example.com")];
    let mut stale_probe = egress_probe(NodeSubscriptionRegion::Taiwan, "TW", "203.0.113.30");
    stale_probe.last_success_at = Some("2026-04-24T00:00:00Z".to_string());

    let mut probes = BTreeMap::new();
    probes.insert("n1".to_string(), stale_probe);

    let region_map = build_mihomo_base_region_map(&nodes, &probes);

    assert_eq!(
        region_map.get("tokyo-a"),
        Some(&NodeSubscriptionRegion::Taiwan)
    );
}

#[test]
fn build_mihomo_base_region_map_keeps_invalidated_probe_region_other_without_slug_fallback() {
    let nodes = vec![node("n1", "tokyo-a", "tokyo-a.example.com")];
    let probe = NodeEgressProbeState {
        subscription_region: NodeSubscriptionRegion::Other,
        checked_at: "2026-04-24T01:00:00Z".to_string(),
        selected_public_ip: Some("198.51.100.9".to_string()),
        classification_invalidated_at: Some("2026-04-24T01:00:00Z".to_string()),
        error_summary: Some("country.is lookup failed".to_string()),
        ..NodeEgressProbeState::default()
    };

    let mut probes = BTreeMap::new();
    probes.insert("n1".to_string(), probe);

    let region_map = build_mihomo_base_region_map(&nodes, &probes);

    assert_eq!(
        region_map.get("tokyo-a"),
        Some(&NodeSubscriptionRegion::Other)
    );
}

#[test]
fn app_proxy_group_shape_only_matches_hidden_wrapper_name() {
    assert!(!has_app_proxy_group_shape(&["🚀 节点选择".to_string()]));
    assert!(has_app_proxy_group_shape(&["💎 节点选择".to_string()]));
}

#[test]
fn ss2022_password_is_percent_encoded_in_raw_uri_userinfo_plain_form() {
    let u = user("u1", "alice");
    let n = node("n1", "node-1", "example.com");

    // A valid base64 string that includes '+' and '/' to exercise percent encoding.
    let server_psk_b64 = "+/v7+/v7+/v7+/v7+/v7+w==";

    let ep = endpoint_ss("e1", "n1", "ss", 443, server_psk_b64);
    let m = membership("u1", "n1", "e1");

    let lines = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap();
    assert_eq!(lines.len(), 1);
    let uri = &lines[0];

    assert!(uri.contains("ss://2022-blake3-aes-128-gcm:"));
    assert!(uri.contains("%2B"));
    assert!(uri.contains("%2F"));
    assert!(uri.contains("%3D"));
    assert!(uri.contains("%3A"));
    assert!(uri.contains("@example.com:443"));
}

#[test]
fn name_is_url_encoded_in_fragment_space_is_percent_20_not_plus() {
    let u = user("u1", "hello world");
    let n = node("n1", "node-1", "example.com");
    let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
    let m = membership("u1", "n1", "e1");

    let lines = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap();
    assert_eq!(lines.len(), 1);
    let uri = &lines[0];

    assert!(uri.contains("#hello%20world-"));
    assert!(!uri.contains("#hello+world"));
}

#[test]
fn empty_node_access_host_is_error() {
    let u = user("u1", "alice");
    let n = node("n1", "node-1", "");
    let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
    let m = membership("u1", "n1", "e1");

    let err = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap_err();
    assert_eq!(
        err,
        SubscriptionError::EmptyNodeAccessHost {
            node_id: "n1".to_string(),
            endpoint_id: "e1".to_string(),
        }
    );
}

#[test]
fn whitespace_node_access_host_is_error_for_mihomo_yaml() {
    let u = user("u1", "alice");
    let n = node("n1", "node-1", "   ");
    let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
    let m = membership("u1", "n1", "e1");
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let err = build_mihomo_yaml(SEED, &u, &[m], &[ep], &[n], &profile).unwrap_err();
    assert_eq!(
        err,
        SubscriptionError::EmptyNodeAccessHost {
            node_id: "n1".to_string(),
            endpoint_id: "e1".to_string(),
        }
    );
}

#[test]
fn vless_server_names_empty_is_error() {
    let u = user("u1", "alice");
    let n = node("n1", "node-1", "example.com");
    let meta = serde_json::json!({
      "reality": {"dest": "example.com:443", "server_names": [], "fingerprint": "chrome"},
      "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"},
      "short_ids": ["0123456789abcdef"],
      "active_short_id": "0123456789abcdef"
    });
    let ep = endpoint_vless("e1", "n1", "vless", 443, meta);
    let m = membership("u1", "n1", "e1");

    let err = build_raw_lines(SEED, &u, &[m], &[ep], &[n]).unwrap_err();
    assert_eq!(
        err,
        SubscriptionError::VlessRealityServerNamesEmpty {
            endpoint_id: "e1".to_string(),
        }
    );
}

#[test]
fn build_clash_yaml_has_proxies_and_derived_secrets() {
    let u = user("u1", "alice");
    let n = node("n1", "node-1", "example.com");

    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];

    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];

    let yaml = build_clash_yaml(SEED, &u, &memberships, &endpoints, &[n]).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let proxies = v
        .get("proxies")
        .and_then(|x| x.as_sequence())
        .expect("proxies must be a list");
    assert_eq!(proxies.len(), 2);

    let ss = proxies
        .iter()
        .find(|p| p.get("type") == Some(&Value::String("ss".to_string())))
        .unwrap();
    assert_eq!(
        ss.get("server"),
        Some(&Value::String("example.com".to_string()))
    );
    assert_eq!(ss.get("port"), Some(&Value::Number(443.into())));
    assert_eq!(
        ss.get("cipher"),
        Some(&Value::String(
            SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string()
        ))
    );

    let expected_user_psk =
        crate::credentials::derive_ss2022_user_psk_b64(SEED, "u1", u.credential_epoch).unwrap();
    let expected_password = format!("AAAAAAAAAAAAAAAAAAAAAA==:{expected_user_psk}");
    assert_eq!(ss.get("password"), Some(&Value::String(expected_password)));
    assert_eq!(ss.get("udp"), Some(&Value::Bool(true)));

    let vless = proxies
        .iter()
        .find(|p| p.get("type") == Some(&Value::String("vless".to_string())))
        .unwrap();
    assert_eq!(
        vless.get("server"),
        Some(&Value::String("example.com".to_string()))
    );
    assert_eq!(vless.get("port"), Some(&Value::Number(8443.into())));

    let expected_uuid =
        crate::credentials::derive_vless_uuid(SEED, "u1", u.credential_epoch).unwrap();
    assert_eq!(vless.get("uuid"), Some(&Value::String(expected_uuid)));
}

#[test]
fn empty_membership_list_produces_empty_output() {
    let u = user("u1", "alice");
    let n = node("n1", "node-1", "example.com");
    let ep = endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA==");

    let out = build_raw_lines(SEED, &u, &[], &[ep], &[n]).unwrap();
    assert!(out.is_empty());

    let out_raw = build_raw_text(SEED, &u, &[], &[], &[]).unwrap();
    assert_eq!(out_raw, "");

    let out_b64 = build_base64(SEED, &u, &[], &[], &[]).unwrap();
    assert_eq!(out_b64, "");
}

#[test]
fn order_is_deterministic() {
    let u = user("u1", "alice");
    let n = node("n1", "node-1", "example.com");

    let ep1 = endpoint_ss("e1", "n1", "tag-2", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
    let ep2 = endpoint_ss("e2", "n1", "tag-1", 443, "AAAAAAAAAAAAAAAAAAAAAA==");
    let m1 = membership("u1", "n1", "e1");
    let m2 = membership("u1", "n1", "e2");

    let out1 = build_raw_lines(
        SEED,
        &u,
        &[m2.clone(), m1.clone()],
        &[ep2.clone(), ep1.clone()],
        std::slice::from_ref(&n),
    )
    .unwrap();
    let out2 = build_raw_lines(SEED, &u, &[m1, m2], &[ep1, ep2], &[n]).unwrap();

    assert_eq!(out1, out2);
    assert_eq!(out1.len(), 2);
}

#[test]
fn build_mihomo_yaml_preserves_mixin_defined_proxies_and_outer_group() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxies:
  - name: "custom-direct"
    type: ss
    server: custom.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: "x:y"
    udp: true
proxy-groups:
  - name: "🛣️ JP/HK/TW"
    type: url-test
    use: []
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
providerB:
  type: http
  path: ./provider-b.yaml
  url: https://example.com/b
"#
        .to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();

    let proxies = v
        .get("proxies")
        .and_then(|x| x.as_sequence())
        .expect("proxies must be a list");
    let names = proxies
        .iter()
        .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
        .collect::<std::collections::BTreeSet<_>>();

    assert!(names.contains("Tokyo-A-reality"));
    assert!(names.contains("Tokyo-A-ss"));
    assert!(names.contains("Tokyo-A-ss-chain"));
    assert!(names.contains("Tokyo-A-reality-chain"));
    assert!(names.contains("custom-direct"));
    assert!(!names.contains("Tokyo-A-JP"));
    assert!(!names.contains("Tokyo-A-HK"));
    assert!(!names.contains("Tokyo-A-KR"));
    assert!(!names.contains("Tokyo-A-TW"));

    let proxy_groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must be a list");
    let relay_group = proxy_groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ example-com"))
        .expect("missing per-server relay group");
    assert!(
        !proxy_groups
            .iter()
            .any(|g| { g.get("name").and_then(Value::as_str) == Some(MIHOMO_LEGACY_OUTER_GROUP) }),
        "legacy outer group should be removed from rendered output"
    );
    let use_names = relay_group
        .get("use")
        .and_then(Value::as_sequence)
        .expect("use must be sequence")
        .iter()
        .filter_map(Value::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        use_names,
        std::collections::BTreeSet::from(["providerA", "providerB"])
    );
    assert_eq!(
        relay_group.get("url").and_then(Value::as_str),
        Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
    );
    let japan_group = proxy_groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🌟 Japan"))
        .expect("region Japan source group should exist");
    assert_eq!(
        japan_group.get("type"),
        Some(&Value::String("fallback".to_string()))
    );
    assert_eq!(japan_group.get("hidden"), Some(&Value::Bool(true)));
    let japan_refs = japan_group
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("region Japan source group should wrap visible Japan group")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(japan_refs, vec!["🔒 Japan"]);

    let japan_alias = proxy_groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🔒 Japan"))
        .expect("visible Japan group should exist");
    assert_eq!(
        japan_alias.get("type"),
        Some(&Value::String("select".to_string()))
    );
    assert_eq!(japan_alias.get("hidden"), None);
    let japan_alias_refs = japan_alias
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("visible Japan group should expose region leaf proxies")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(japan_alias_refs, vec!["Tokyo-A-reality"]);

    let landing = proxy_groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🛬 Tokyo-A"))
        .expect("landing group must exist");
    let landing_refs = landing
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("landing proxies must exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(landing_refs, vec!["Tokyo-A-reality"]);
}

#[test]
fn build_mihomo_provider_yaml_moves_generated_system_proxies_to_provider_payload() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🔒 高质量"
    type: select
    use: ["providerA"]
    exclude-filter: "剩余|到期"
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();

    let proxy_names = root
        .get("proxies")
        .and_then(Value::as_sequence)
        .unwrap()
        .iter()
        .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(
        proxy_names.is_empty(),
        "provider main config should move generated system proxies into provider payload"
    );

    let provider_map = root
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .unwrap();
    let system_provider = provider_map
        .get(Value::String(MIHOMO_SYSTEM_PROVIDER_NAME.to_string()))
        .and_then(Value::as_mapping)
        .expect("provider route should inject xp-system-generated");
    assert_eq!(
        system_provider
            .get(Value::String("url".to_string()))
            .and_then(Value::as_str),
        Some("https://sub.example.com/api/sub/token/mihomo/provider/system")
    );

    let proxy_groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .unwrap();
    let landing_group = proxy_groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🛬 Tokyo-A"))
        .expect("provider route should keep landing group");
    assert!(landing_group.get("proxies").is_none());
    assert_eq!(
        landing_group
            .get("use")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec![MIHOMO_SYSTEM_PROVIDER_NAME]
    );
    let landing_filter = landing_group
        .get("filter")
        .and_then(Value::as_str)
        .expect("landing group should filter provider-hosted chain proxies");
    assert!(landing_filter.contains("Tokyo\\-A\\-ss\\-chain"));
    assert!(landing_filter.contains("Tokyo\\-A\\-reality\\-chain"));
    assert!(!landing_filter.contains("Tokyo\\-A\\-ss|"));
    assert!(!landing_filter.contains("Tokyo\\-A\\-reality|"));

    let japan_group = proxy_groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🌟 Japan"))
        .expect("provider route should keep region source group");
    assert_eq!(
        japan_group.get("type"),
        Some(&Value::String("fallback".to_string()))
    );
    assert_eq!(japan_group.get("hidden"), Some(&Value::Bool(true)));
    assert_eq!(
        japan_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec!["🔒 Japan"]
    );

    let japan_visible_group = proxy_groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 Japan"))
        .expect("provider route should keep visible region group");
    let japan_filter = japan_visible_group
        .get("filter")
        .and_then(Value::as_str)
        .expect("visible Japan group should filter provider-hosted system proxies");
    assert!(japan_filter.contains("日本|🇯🇵|Japan|JP"));
    assert!(japan_filter.contains("Tokyo\\-A\\-reality"));
    assert!(!japan_filter.contains("Tokyo\\-A\\-ss|"));
    let japan_exclude_filter = japan_visible_group
        .get("exclude-filter")
        .and_then(Value::as_str)
        .expect("visible Japan group should exclude provider-hosted direct ss proxies");
    assert!(japan_exclude_filter.contains("Tokyo\\-A\\-ss"));
    assert!(!japan_exclude_filter.contains("Tokyo\\-A\\-reality"));
    assert_eq!(
        japan_visible_group
            .get("use")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec![MIHOMO_SYSTEM_PROVIDER_NAME, "providerA"]
    );
    assert_eq!(
        japan_visible_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        None
    );

    let high_quality_group = proxy_groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 高质量"))
        .expect("provider route should keep high quality group");
    assert_eq!(
        high_quality_group
            .get("use")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec![MIHOMO_SYSTEM_PROVIDER_NAME, "providerA"]
    );
    let high_quality_filter = high_quality_group
        .get("filter")
        .and_then(Value::as_str)
        .expect("high quality group should include provider-hosted reality access");
    assert!(high_quality_filter.contains("Tokyo\\-A\\-reality"));
    assert!(!high_quality_filter.contains("Tokyo\\-A\\-ss|"));
    let high_quality_exclude_filter = high_quality_group
        .get("exclude-filter")
        .and_then(Value::as_str)
        .expect("high quality group should exclude provider-hosted direct ss proxies");
    assert!(high_quality_exclude_filter.contains("剩余|到期"));
    assert!(high_quality_exclude_filter.contains("Tokyo\\-A\\-ss"));
    assert!(!high_quality_exclude_filter.contains("Tokyo\\-A\\-reality"));
}

#[test]
fn build_mihomo_provider_yaml_groups_relay_by_access_host() {
    let u = user("u1", "alice");
    let n1 = node_with_api_base(
        "n1",
        "Tokyo A",
        "shared.example.com",
        "https://tokyo-a.example.com",
    );
    let n2 = node_with_api_base(
        "n2",
        "Tokyo B",
        "shared.example.com",
        "https://tokyo-b.example.com",
    );
    let n3 = node_with_api_base(
        "n3",
        "Seoul A",
        "seoul.example.com",
        "https://seoul-a.example.com",
    );
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_ss("e3", "n3", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
    ];
    let memberships = vec![
        membership("u1", "n1", "e1"),
        membership("u1", "n2", "e2"),
        membership("u1", "n3", "e3"),
    ];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n1, n2, n3],
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    let relay_names = groups
        .iter()
        .filter_map(|group| group.get("name").and_then(Value::as_str))
        .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
        .collect::<Vec<_>>();
    assert_eq!(
        relay_names,
        vec!["🛣️ seoul-example-com", "🛣️ shared-example-com"]
    );
    let relay_url = |name: &str| {
        groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|group| group.get("url"))
            .and_then(Value::as_str)
    };
    assert_eq!(
        relay_url("🛣️ shared-example-com"),
        Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
    );
    assert_eq!(
        relay_url("🛣️ seoul-example-com"),
        Some("https://seoul-a.example.com/api/health")
    );

    let system_yaml = build_mihomo_provider_system_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[
            node_with_api_base(
                "n1",
                "Tokyo A",
                "shared.example.com",
                "https://tokyo-a.example.com",
            ),
            node_with_api_base(
                "n2",
                "Tokyo B",
                "shared.example.com",
                "https://tokyo-b.example.com",
            ),
            node_with_api_base(
                "n3",
                "Seoul A",
                "seoul.example.com",
                "https://seoul-a.example.com",
            ),
        ],
    )
    .unwrap();
    let system_root: Value = serde_yaml::from_str(&system_yaml).unwrap();
    let proxy_dialer = |name: &str| {
        system_root
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies
                    .iter()
                    .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some(name))
            })
            .and_then(|proxy| proxy.get("dialer-proxy"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };

    assert_eq!(
        proxy_dialer("Tokyo-A-ss-chain").as_deref(),
        Some("🛣️ shared-example-com")
    );
    assert_eq!(
        proxy_dialer("Tokyo-B-ss-chain").as_deref(),
        Some("🛣️ shared-example-com")
    );
    assert_eq!(
        proxy_dialer("Seoul-A-ss-chain").as_deref(),
        Some("🛣️ seoul-example-com")
    );
}

#[test]
fn build_mihomo_provider_yaml_uses_default_health_when_api_base_is_loopback() {
    let u = user("u1", "alice");
    let n = node_with_api_base(
        "n1",
        "Tokyo A",
        "relay.example.com",
        "https://127.0.0.1:62416",
    );
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let relay = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups.iter().find(|group| {
                group.get("name").and_then(Value::as_str) == Some("🛣️ relay-example-com")
            })
        })
        .expect("relay group should exist");
    assert_eq!(
        relay.get("url").and_then(Value::as_str),
        Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
    );
    assert!(!yaml.contains("127.0.0.1"));
}

#[test]
fn build_mihomo_provider_yaml_uses_api_health_when_shared_access_host_has_one_api_base() {
    let u = user("u1", "alice");
    let n1 = node_with_api_base(
        "n1",
        "Tokyo A",
        "shared.example.com",
        "https://shared-api.example.com",
    );
    let n2 = node_with_api_base(
        "n2",
        "Tokyo B",
        "shared.example.com",
        "https://shared-api.example.com",
    );
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n2", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n1, n2],
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let relay = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups.iter().find(|group| {
                group.get("name").and_then(Value::as_str) == Some("🛣️ shared-example-com")
            })
        })
        .expect("shared relay group should exist");
    assert_eq!(
        relay.get("url").and_then(Value::as_str),
        Some("https://shared-api.example.com/api/health")
    );
}

#[test]
fn build_mihomo_provider_yaml_uses_managed_default_vless_port_for_relay_url() {
    let u = user("u1", "alice");
    let n = node_with_api_base(
        "n1",
        "Hinet",
        "hinet-ep.example.com",
        "https://hinet-xp.example.com",
    );
    let endpoints = vec![endpoint_vless(
        "e1",
        "n1",
        "vless-vision-e1",
        53844,
        vless_meta("example.com:443", &["example.com"], true),
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: String::new(),
    };

    let yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let relay = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups.iter().find(|group| {
                group.get("name").and_then(Value::as_str) == Some("🛣️ hinet-dash-ep-example-com")
            })
        })
        .expect("relay group should exist");
    assert_eq!(
        relay.get("url").and_then(Value::as_str),
        Some("https://hinet-ep.example.com:53844/generate_204")
    );
}

#[test]
fn build_mihomo_provider_yaml_uses_node_managed_vless_port_even_if_user_only_has_ss_membership() {
    let u = user("u1", "alice");
    let n = node_with_api_base(
        "n1",
        "Hinet",
        "hinet-ep.example.com",
        "https://hinet-xp.example.com",
    );
    let endpoints = vec![
        endpoint_vless(
            "e1",
            "n1",
            "vless-vision-e1",
            53844,
            vless_meta("example.com:443", &["example.com"], true),
        ),
        endpoint_ss("e2", "n1", "ss", 53843, "AAAAAAAAAAAAAAAAAAAAAA=="),
    ];
    let memberships = vec![membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: String::new(),
    };

    let yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let relay = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups.iter().find(|group| {
                group.get("name").and_then(Value::as_str) == Some("🛣️ hinet-dash-ep-example-com")
            })
        })
        .expect("relay group should exist");
    assert_eq!(
        relay.get("url").and_then(Value::as_str),
        Some("https://hinet-ep.example.com:53844/generate_204")
    );
}

#[test]
fn build_mihomo_provider_yaml_ignores_non_managed_vless_for_relay_url() {
    let u = user("u1", "alice");
    let n = node_with_api_base(
        "n1",
        "Hinet",
        "hinet-ep.example.com",
        "https://hinet-xp.example.com",
    );
    let endpoints = vec![endpoint_vless(
        "e1",
        "n1",
        "vless-vision-e1",
        53844,
        vless_meta("example.com:443", &["example.com"], false),
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: String::new(),
    };

    let yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let relay = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups.iter().find(|group| {
                group.get("name").and_then(Value::as_str) == Some("🛣️ hinet-dash-ep-example-com")
            })
        })
        .expect("relay group should exist");
    assert_eq!(
        relay.get("url").and_then(Value::as_str),
        Some("https://hinet-xp.example.com/api/health")
    );
}

#[test]
fn build_mihomo_provider_yaml_keeps_relay_group_name_stable_for_same_access_host() {
    let u = user("u1", "alice");
    let n1 = node_with_api_base(
        "n1",
        "Tokyo B",
        "shared.example.com",
        "https://tokyo-b.example.com",
    );
    let n2 = node_with_api_base(
        "n2",
        "Aardvark",
        "shared.example.com",
        "https://aardvark.example.com",
    );
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
    ];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml:
            "providerA:\n  type: http\n  path: ./provider-a.yaml\n  url: https://example.com/a\n"
                .to_string(),
    };

    let relay_names_for = |memberships: Vec<NodeUserEndpointMembership>| {
        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1.clone(), n2.clone()],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        root.get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist")
            .iter()
            .filter_map(|group| group.get("name").and_then(Value::as_str))
            .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        relay_names_for(vec![membership("u1", "n1", "e1")]),
        vec!["🛣️ shared-example-com"]
    );
    assert_eq!(
        relay_names_for(vec![
            membership("u1", "n1", "e1"),
            membership("u1", "n2", "e2"),
        ]),
        vec!["🛣️ shared-example-com"]
    );
}

#[test]
fn build_mihomo_provider_yaml_keeps_relay_group_name_stable_for_access_host_slug_collisions() {
    let u = user("u1", "alice");
    let n1 = node_with_api_base(
        "n1",
        "Dot Host",
        "a.b.example.com",
        "https://dot.example.com",
    );
    let n2 = node_with_api_base(
        "n2",
        "Dash Host",
        "a-b.example.com",
        "https://dash.example.com",
    );
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
    ];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml:
            "providerA:\n  type: http\n  path: ./provider-a.yaml\n  url: https://example.com/a\n"
                .to_string(),
    };

    let relay_names_for = |memberships: Vec<NodeUserEndpointMembership>| {
        let yaml = build_mihomo_provider_yaml(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &[n1.clone(), n2.clone()],
            &profile,
            "https://sub.example.com/api/sub/token/mihomo/provider/system",
        )
        .unwrap();
        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        root.get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups must exist")
            .iter()
            .filter_map(|group| group.get("name").and_then(Value::as_str))
            .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
            .map(str::to_string)
            .collect::<std::collections::BTreeSet<_>>()
    };

    assert_eq!(
        relay_names_for(vec![membership("u1", "n1", "e1")]),
        std::collections::BTreeSet::from(["🛣️ a-b-example-com".to_string()])
    );
    assert_eq!(
        relay_names_for(vec![membership("u1", "n2", "e2")]),
        std::collections::BTreeSet::from(["🛣️ a-dash-b-example-com".to_string()])
    );
    assert_eq!(
        relay_names_for(vec![
            membership("u1", "n1", "e1"),
            membership("u1", "n2", "e2"),
        ]),
        std::collections::BTreeSet::from([
            "🛣️ a-b-example-com".to_string(),
            "🛣️ a-dash-b-example-com".to_string(),
        ])
    );
}

#[test]
fn build_mihomo_yaml_keeps_generated_relay_group_ref_in_extra_proxy_dialer_proxy() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "relay.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ relay-example-com
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom extra proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("🛣️ relay-example-com")
    );
}

#[test]
fn validate_mihomo_profile_via_provider_render_rejects_provider_payload_proxy_ref_in_main_config() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "relay.example.com");
    let endpoints = vec![endpoint_vless(
        "e1",
        "n1",
        "vless",
        8443,
        serde_json::json!({
          "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
          "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
          "short_ids": ["0123456789abcdef"],
          "active_short_id": "0123456789abcdef"
        }),
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: Tokyo-A-reality
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let err = validate_mihomo_profile_via_provider_render(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probe_map(&[("n1", NodeSubscriptionRegion::Japan)]),
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .expect_err("main config must not silently remap provider payload proxy references");
    let message = err.to_string();
    assert!(
        message.contains("Tokyo-A-reality"),
        "expected provider payload proxy ref to be rejected, got: {message}"
    );
}

#[test]
fn build_mihomo_yaml_generated_relay_group_wins_custom_name_collision() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "relay.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ relay-example-com"
    type: select
    proxies: ["DIRECT"]
  - name: Auto
    type: select
    proxies: ["🛣️ relay-example-com"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    let relay_groups = groups
        .iter()
        .filter(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ relay-example-com"))
        .collect::<Vec<_>>();
    assert_eq!(relay_groups.len(), 1);
    assert_eq!(
        relay_groups[0].get("type").and_then(Value::as_str),
        Some("url-test")
    );
    let auto_refs = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Auto refs must exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(auto_refs, vec!["🛣️ relay-example-com"]);
}

#[test]
fn build_mihomo_provider_yaml_rejects_reserved_proxy_name_collision() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "relay.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: Auto
    type: select
    proxies: ["🛣️ relay-example-com"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: r#"
- name: "🛣️ relay-example-com"
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
- name: Custom-chain
  type: ss
  server: chained.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: "🛣️ relay-example-com"
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let err = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .expect_err("reserved proxy name collision must be rejected");
    assert_eq!(
        err,
        SubscriptionError::MihomoReservedProxyNameConflict {
            name: "🛣️ relay-example-com".to_string(),
        }
    );
}

#[test]
fn build_mihomo_provider_yaml_limits_relay_groups_to_subscribed_nodes() {
    let u = user("u1", "alice");
    let n1 = node_with_api_base(
        "n1",
        "Aardvark",
        "shared.example.com",
        "https://unsubscribed.example.com",
    );
    let n2 = node_with_api_base(
        "n2",
        "Tokyo B",
        "shared.example.com",
        "https://subscribed.example.com",
    );
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
    ];
    let memberships = vec![membership("u1", "n2", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n1.clone(), n2.clone()],
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    let relay_names = groups
        .iter()
        .filter_map(|group| group.get("name").and_then(Value::as_str))
        .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
        .collect::<Vec<_>>();
    assert_eq!(relay_names, vec!["🛣️ shared-example-com"]);
    let relay = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ shared-example-com"))
        .expect("subscribed relay group should exist");
    assert_eq!(
        relay.get("url").and_then(Value::as_str),
        Some("https://subscribed.example.com/api/health")
    );

    let system_yaml =
        build_mihomo_provider_system_yaml(SEED, &u, &memberships, &endpoints, &[n1, n2]).unwrap();
    let system_root: Value = serde_yaml::from_str(&system_yaml).unwrap();
    let chain_dialer = system_root
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Tokyo-B-ss-chain"))
        })
        .and_then(|proxy| proxy.get("dialer-proxy"))
        .and_then(Value::as_str);
    assert_eq!(chain_dialer, Some("🛣️ shared-example-com"));
}

#[test]
fn build_mihomo_provider_yaml_avoids_legacy_region_relay_alias_names() {
    let u = user("u1", "alice");
    let n = node("n1", "Japan", "jp.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: Auto
    type: select
    proxies: ["🛣️ Japan", "🔒 Japan"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let err = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        std::slice::from_ref(&n),
        &probes,
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .expect_err("legacy region relay aliases must be rejected");
    assert!(
        err.to_string().contains("🛣️ Japan"),
        "expected legacy alias in error: {err}"
    );

    let system_yaml =
        build_mihomo_provider_system_yaml(SEED, &u, &memberships, &endpoints, &[n]).unwrap();
    let system_root: Value = serde_yaml::from_str(&system_yaml).unwrap();
    let chain_dialer = system_root
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Japan-ss-chain"))
        })
        .and_then(|proxy| proxy.get("dialer-proxy"))
        .and_then(Value::as_str);
    assert_eq!(chain_dialer, Some("🛣️ jp-example-com"));
}

#[test]
fn build_mihomo_provider_yaml_deduplicates_disambiguated_relay_group_names() {
    let u = user("u1", "alice");
    let n1 = node("n1", "Japan", "jp.example.com");
    let n2 = node("n2", "relay-Japan", "relay-jp.example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_ss("e2", "n2", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n2", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let probes = probe_map(&[
        ("n1", NodeSubscriptionRegion::Japan),
        ("n2", NodeSubscriptionRegion::Japan),
    ]);
    let yaml = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n1.clone(), n2.clone()],
        &probes,
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    let relay_names = groups
        .iter()
        .filter_map(|group| group.get("name").and_then(Value::as_str))
        .filter(|name| name.starts_with(MIHOMO_RELAY_GROUP_PREFIX))
        .collect::<Vec<_>>();
    assert_eq!(
        relay_names,
        vec!["🛣️ jp-example-com", "🛣️ relay-dash-jp-example-com"]
    );

    let system_yaml =
        build_mihomo_provider_system_yaml(SEED, &u, &memberships, &endpoints, &[n1, n2]).unwrap();
    let system_root: Value = serde_yaml::from_str(&system_yaml).unwrap();
    let chain_dialer = |name: &str| {
        system_root
            .get("proxies")
            .and_then(Value::as_sequence)
            .and_then(|proxies| {
                proxies
                    .iter()
                    .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some(name))
            })
            .and_then(|proxy| proxy.get("dialer-proxy"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    assert_eq!(
        chain_dialer("Japan-ss-chain").as_deref(),
        Some("🛣️ jp-example-com")
    );
    assert_eq!(
        chain_dialer("relay-Japan-ss-chain").as_deref(),
        Some("🛣️ relay-dash-jp-example-com")
    );
}

#[test]
fn build_mihomo_provider_yaml_injects_default_aggregate_groups() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules:\n  - MATCH,🚀 节点选择\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: file
  path: ./provider-a.yaml
"#
        .to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    let group_refs = |name: &str| {
        groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
    };

    assert_eq!(group_refs("💎 高质量"), vec!["🔒 高质量", "🤯 All"]);
    assert_eq!(
        groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 高质量"))
            .and_then(|group| group.get("use"))
            .and_then(Value::as_sequence)
            .expect("high quality group should keep provider candidates")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec![MIHOMO_SYSTEM_PROVIDER_NAME, "providerA"]
    );
    assert!(group_refs("🤯 All").contains(&"🤯 Japan"));
    assert_eq!(
        group_refs("🚀 节点选择"),
        vec![
            "🌟 Japan",
            "🌟 HongKong",
            "🌟 Taiwan",
            "🌟 Korea",
            "🌟 Singapore",
            "🌟 US",
            "🌟 Other",
            "🛬 Tokyo-A",
            "💎 高质量",
        ]
    );
    assert!(
        root.get("rules")
            .and_then(Value::as_sequence)
            .and_then(|rules| rules.first())
            .and_then(Value::as_str)
            .is_some_and(|rule| rule == "MATCH,🚀 节点选择")
    );
}

#[test]
fn build_mihomo_provider_yaml_places_visible_region_block_after_quality_groups() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "💎 高质量"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🔒 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🔒 Singapore"
    type: select
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🔒 US"
    type: select
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let visible_names = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .unwrap()
        .iter()
        .filter(|group| group.get("hidden").and_then(Value::as_bool) != Some(true))
        .filter_map(|group| group.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert_eq!(
        &visible_names[..9],
        &[
            "🔒 高质量",
            "🔒 Japan",
            "🔒 HongKong",
            "🔒 Taiwan",
            "🔒 Korea",
            "🔒 Singapore",
            "🔒 US",
            "🔒 Other",
            "🛬 Tokyo-A",
        ]
    );

    let singapore_source_group = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("🌟 Singapore"))
        })
        .expect("🌟 Singapore group should be rebuilt by the system");
    assert_eq!(
        singapore_source_group
            .get("hidden")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        singapore_source_group
            .get("proxies")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec!["🔒 Singapore"]
    );
}

#[test]
fn build_mihomo_provider_yaml_moves_hidden_relay_groups_after_system_visible_groups() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "relay.example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .expect("build mihomo provider yaml should succeed");
    let root: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups should exist");

    let index_of = |name: &str| {
        groups
            .iter()
            .position(|group| group.get("name").and_then(Value::as_str) == Some(name))
            .unwrap_or(usize::MAX)
    };

    assert!(index_of("🛣️ relay-example-com") > index_of("🔒 落地"));
    assert!(index_of("🛣️ relay-example-com") > index_of("🤯 All"));
    assert!(index_of("🛣️ relay-example-com") > index_of("🚀 节点选择"));
}

#[test]
fn build_mihomo_provider_yaml_locks_system_proxy_group_sequence() {
    let u = user("u1", "alice");
    let nodes = vec![
        node("n1", "Tokyo A", "relay-a.example.com"),
        node("n2", "Osaka B", "relay-b.example.com"),
    ];
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
        endpoint_ss("e3", "n2", "ss", 443, "BBBBBBBBBBBBBBBBBBBBBB=="),
        endpoint_vless(
            "e4",
            "n2",
            "vless",
            9443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![
        membership("u1", "n1", "e1"),
        membership("u1", "n1", "e2"),
        membership("u1", "n2", "e3"),
        membership("u1", "n2", "e4"),
    ];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Custom Select"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let probes = probe_map(&[
        ("n1", NodeSubscriptionRegion::Japan),
        ("n2", NodeSubscriptionRegion::Korea),
    ]);
    let yaml = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &nodes,
        &probes,
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .expect("build mihomo provider yaml should succeed");
    let root: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let names = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist")
        .iter()
        .filter_map(|group| group.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert_eq!(
        &names[0..25],
        &[
            "🔒 高质量",
            "💎 高质量",
            "🔒 Japan",
            "🔒 HongKong",
            "🔒 Taiwan",
            "🔒 Korea",
            "🔒 Singapore",
            "🔒 US",
            "🔒 Other",
            "🌟 Japan",
            "🌟 HongKong",
            "🌟 Taiwan",
            "🌟 Korea",
            "🌟 Singapore",
            "🌟 US",
            "🌟 Other",
            "🤯 Japan",
            "🤯 HongKong",
            "🤯 Taiwan",
            "🤯 Korea",
            "🤯 Singapore",
            "🤯 US",
            "🤯 Other",
            "🛬 Osaka-B",
            "🛬 Tokyo-A",
        ]
    );
    assert_eq!(
        &names[25..30],
        &[
            "🔒 落地",
            "🤯 All",
            "🚀 节点选择",
            "💎 节点选择",
            "Custom Select"
        ]
    );
    assert!(
        names.ends_with(&["🛣️ relay-dash-a-example-com", "🛣️ relay-dash-b-example-com",]),
        "hidden relay groups must stay at the tail"
    );
}

#[test]
fn build_mihomo_provider_yaml_keeps_unprobed_singapore_nodes_in_other_group() {
    let u = user("u1", "alice");
    let n = node("n1", "Singapore A", "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 Other"
    type: select
    hidden: true
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &BTreeMap::new(),
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");

    let other_refs = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 Other"))
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .map(|seq| seq.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(
        other_refs.is_empty(),
        "provider visible Other group may rely on filter/use only when no explicit landing proxies exist"
    );

    let singapore_refs = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 Singapore"))
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .map(|seq| seq.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(
        singapore_refs.is_empty(),
        "unprobed Singapore nodes should stay in Other until a successful probe exists"
    );
}

#[test]
fn build_mihomo_yaml_helper_order_keeps_other_aliases() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 US
    - 🌟 Other
    - Manual
port: 0
proxy-groups:
  - name: "Auto"
    type: select
    proxies: ["Manual", "🌟 Other", "🌟 US"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: r#"
- name: Manual
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let refs = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Auto proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(refs, vec!["🌟 US", "🌟 Other", "Manual"]);
}

#[test]
fn known_non_other_region_filter_avoids_matching_embedded_us_fragments() {
    let regex = Regex::new(&known_non_other_region_filter()).unwrap();

    assert!(regex.is_match("US-1"));
    assert!(regex.is_match("Singapore A"));
    assert!(!regex.is_match("AUS-1"));
    assert!(!regex.is_match("PLUS"));
}

#[test]
fn build_mihomo_provider_system_yaml_contains_all_system_proxies() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];

    let yaml = build_mihomo_provider_system_yaml(SEED, &u, &memberships, &endpoints, &[n]).unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let proxy_names = root
        .get("proxies")
        .and_then(Value::as_sequence)
        .unwrap()
        .iter()
        .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert_eq!(
        proxy_names,
        vec![
            "Tokyo-A-reality",
            "Tokyo-A-ss-chain",
            "Tokyo-A-reality-chain",
            "Tokyo-A-ss"
        ]
    );
}

#[test]
fn build_mihomo_provider_yaml_preserves_direct_refs_via_system_provider() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🛬 Legacy-A
    - Legacy-A-reality
    - Legacy-A-ss
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies:
      - 🛬 Legacy-A
      - Legacy-A-reality
      - Legacy-A-ss
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let err = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .expect_err("legacy landing and direct proxy refs must be rejected");
    let message = err.to_string();
    assert!(
        message.contains("🛬 Legacy-A")
            || message.contains("Legacy-A-reality")
            || message.contains("Legacy-A-ss"),
        "expected legacy direct refs in error: {message}"
    );
}

#[test]
fn build_mihomo_yaml_rejects_non_mapping_template() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: "- not-a-mapping".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let err = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap_err();
    assert_eq!(err, SubscriptionError::MihomoMixinRootNotMapping);
}

#[test]
fn build_mihomo_yaml_adds_missing_outer_group() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Auto"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();

    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must be a sequence");

    assert!(
        groups
            .iter()
            .any(|g| g.get("name").and_then(Value::as_str) == Some("Auto")),
        "non-system groups in template should be preserved"
    );

    let relay = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ example-com"))
        .expect("relay group should be auto-added");
    assert_eq!(
        relay.get("type"),
        Some(&Value::String("url-test".to_string()))
    );
    assert_eq!(
        relay.get("interval"),
        Some(&Value::Number(serde_yaml::Number::from(30)))
    );
    assert_eq!(
        relay.get("timeout"),
        Some(&Value::Number(serde_yaml::Number::from(1000)))
    );
    assert_eq!(
        relay.get("max-failed-times"),
        Some(&Value::Number(serde_yaml::Number::from(1)))
    );
    assert_eq!(relay.get("lazy"), Some(&Value::Bool(false)));
    assert_eq!(
        relay.get("tolerance"),
        Some(&Value::Number(serde_yaml::Number::from(
            MIHOMO_OUTER_URL_TEST_TOLERANCE
        )))
    );
    let use_values = relay
        .get("use")
        .and_then(Value::as_sequence)
        .expect("relay group must include use list")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(use_values, vec!["providerA"]);
    let proxy_values = relay
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("relay group must keep DIRECT fallback")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(proxy_values, vec!["DIRECT"]);
    assert_eq!(
        relay.get("url").and_then(Value::as_str),
        Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
    );
}

#[test]
fn build_mihomo_yaml_injects_relay_filter() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0
rules: []
"
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must be a sequence");

    let relay = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ example-com"))
        .expect("relay group should exist");
    assert_eq!(
        relay.get("filter").and_then(Value::as_str),
        Some(MIHOMO_OUTER_FILTER)
    );
    let filter = relay
        .get("filter")
        .and_then(Value::as_str)
        .expect("relay group should have a filter");
    assert!(filter.contains("Singapore|SG"));
    assert!(!filter.contains("Taiwan"));
    assert!(!filter.contains("台湾"));
    assert!(!filter.contains("台灣"));
    assert!(!filter.contains("🇹🇼"));
}

#[test]
fn build_mihomo_yaml_prunes_missing_proxy_and_provider_refs_when_extras_cleared() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Test"
    type: select
    proxies: ["Alpha-reality", "Tokyo-A-JP", "🛣️ Japan", "🔒 Japan"]
    use: ["providerA", "providerA", "missingProvider"]
  - name: "🛣️ Japan"
    type: url-test
    use: ["providerA"]
  - name: "🔒 Japan"
    type: fallback
    proxies: ["🛣️ Japan"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    assert!(!yaml.contains("providerA"));
    assert!(!yaml.contains("Alpha-reality"));
    assert!(!yaml.contains("Tokyo-A-JP"));

    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let providers = v
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .expect("proxy-providers must exist");
    assert!(providers.is_empty());

    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must be a sequence");

    let test_group = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("Test"))
        .expect("Test group must exist");
    let test_proxy_names = test_group
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("Test proxies must exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(test_proxy_names, vec!["🌟 Japan"]);

    assert!(
        groups
            .iter()
            .all(|g| g.get("name").and_then(Value::as_str) != Some("🛣️ Japan"))
    );

    let japan_group = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🔒 Japan"))
        .expect("compat Japan group should exist");
    let japan_group_refs = japan_group
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("compat Japan proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(japan_group_refs, vec!["DIRECT"]);

    let relay_group = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🛣️ example-com"))
        .expect("relay group must exist");
    let relay_proxy_names = relay_group
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("relay group should fall back to DIRECT")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(relay_proxy_names, vec!["DIRECT"]);
}

#[test]
fn build_mihomo_yaml_reorders_user_groups_using_helper_template_order() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 💎 高质量
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🌟 Korea
    - 🛬 Tokyo-A
    - 💎 高质量
    - Tokyo-A-reality
app-proxy-group:
  proxies:
    - 💎 节点选择
    - 💎 高质量
    - 🗽 大流量
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🎯 全球直连
    - 🛑 全球拦截
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "🗽 大流量"
    type: select
    proxies: ["DIRECT"]
  - name: "🎯 全球直连"
    type: select
    proxies: ["DIRECT"]
  - name: "🛑 全球拦截"
    type: select
    proxies: ["REJECT"]
  - name: "🤯 All"
    type: select
    proxies: ["DIRECT"]
  - name: "💎 节点选择"
    type: select
    proxies: ["🚀 节点选择", "🤯 All"]
  - name: "Simple Auto"
    type: select
    proxies:
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 US
      - 🌟 Singapore
  - name: "Custom Select"
    type: select
    proxies:
      - 🛬 Tokyo-A
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
      - Tokyo-A-reality
  - name: "🐟 漏网之鱼"
    type: select
    proxies:
      - 🛑 全球拦截
      - 🗽 大流量
      - 🌟 US
      - 💎 节点选择
      - 🛣️ JP/HK/TW
      - 🎯 全球直连
      - 💎 高质量
      - 🌟 Singapore
  - name: "🤖 AI"
    type: select
    proxies:
      - 🌟 US
      - 🎯 全球直连
      - 🛣️ JP/HK/TW
      - 🗽 大流量
      - 💎 节点选择
      - 💎 高质量
      - 🌟 Singapore
  - name: "Relay Hidden"
    type: select
    hidden: true
    proxies:
      - Tokyo-A-reality
      - 🔒 落地
      - 🌟 US
      - 🛣️ JP/HK/TW
      - 🛬 Tokyo-A
      - 🌟 Singapore
      - 🎯 全球直连
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must be a sequence");
    let names = groups
        .iter()
        .filter_map(|group| group.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "🔒 高质量",
            "💎 高质量",
            "🔒 Japan",
            "🌟 Japan",
            "🤯 Japan",
            "🔒 HongKong",
            "🌟 HongKong",
            "🤯 HongKong",
            "🔒 Taiwan",
            "🌟 Taiwan",
            "🤯 Taiwan",
            "🔒 Korea",
            "🌟 Korea",
            "🤯 Korea",
            "🔒 Singapore",
            "🌟 Singapore",
            "🤯 Singapore",
            "🔒 US",
            "🌟 US",
            "🤯 US",
            "🔒 Other",
            "🌟 Other",
            "🤯 Other",
            "🔒 落地",
            "🤯 All",
            "🗽 大流量",
            "🎯 全球直连",
            "🛑 全球拦截",
            "Simple Auto",
            "Custom Select",
            "🐟 漏网之鱼",
            "🤖 AI",
            "Relay Hidden",
            "🛬 Tokyo-A",
            "🚀 节点选择",
            "💎 节点选择",
            "🛣️ example-com",
        ]
    );

    let group_proxies = |name: &str| {
        groups
            .iter()
            .find(|g| g.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies must exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        group_proxies("🚀 节点选择"),
        vec![
            "🌟 Japan",
            "🌟 HongKong",
            "🌟 Taiwan",
            "🌟 Korea",
            "🌟 Singapore",
            "🌟 US",
            "🌟 Other",
            "🛬 Tokyo-A",
            "💎 高质量",
        ]
    );
    assert_eq!(
        group_proxies("Simple Auto"),
        vec!["🌟 Singapore", "🌟 US", "💎 高质量"]
    );
    assert_eq!(
        group_proxies("🐟 漏网之鱼"),
        vec![
            "💎 节点选择",
            "💎 高质量",
            "🗽 大流量",
            "🌟 Singapore",
            "🌟 US",
            "🎯 全球直连",
            "🛑 全球拦截",
        ]
    );
    assert_eq!(
        group_proxies("🤖 AI"),
        vec![
            "💎 节点选择",
            "💎 高质量",
            "🗽 大流量",
            "🌟 Singapore",
            "🌟 US",
            "🎯 全球直连",
        ]
    );
    assert_eq!(
        group_proxies("Relay Hidden"),
        vec![
            "🌟 Singapore",
            "🌟 US",
            "🛬 Tokyo-A",
            "Tokyo-A-reality",
            "🔒 落地",
            "🎯 全球直连",
        ]
    );

    for visible_group in [
        "🚀 节点选择",
        "Simple Auto",
        "🐟 漏网之鱼",
        "🤖 AI",
        "Relay Hidden",
    ] {
        let refs = group_proxies(visible_group);
        assert!(!refs.iter().any(|name| {
            matches!(
                *name,
                "🔒 Japan"
                    | "🤯 Japan"
                    | "🛣️ Japan"
                    | "🔒 HongKong"
                    | "🤯 HongKong"
                    | "🛣️ HongKong"
                    | "🔒 Taiwan"
                    | "🤯 Taiwan"
                    | "🛣️ Taiwan"
                    | "🔒 Korea"
                    | "🤯 Korea"
                    | "🛣️ Korea"
                    | "🔒 Singapore"
                    | "🤯 Singapore"
                    | "🛣️ Singapore"
                    | "🔒 US"
                    | "🤯 US"
                    | "🛣️ US"
                    | "🔒 Other"
                    | "🤯 Other"
                    | "🛣️ Other"
            )
        }));
    }

    let star_japan = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🌟 Japan"))
        .expect("🌟 Japan group should exist");
    assert_eq!(star_japan.get("hidden"), Some(&Value::Bool(true)));
    let star_us = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🌟 US"))
        .expect("🌟 US group should exist even when empty");
    let star_us_refs = star_us
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("🌟 US proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(star_us_refs, vec!["🔒 US"]);
    let star_other = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🌟 Other"))
        .expect("🌟 Other group should exist");
    assert_eq!(
        star_other
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("🌟 Other proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec!["🔒 Other"]
    );

    assert!(
        groups
            .iter()
            .all(|g| g.get("name").and_then(Value::as_str) != Some("🛣️ Japan")),
        "legacy relay region aliases should not be generated"
    );
}

#[test]
fn build_mihomo_yaml_remaps_legacy_landing_refs_before_replaying_helper_order() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🛬 Legacy-A
    - 💎 高质量
    - Legacy-A-reality
    - Legacy-A-ss
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies:
      - Legacy-A-ss
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
      - 🛬 Legacy-A
      - Legacy-A-reality
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Custom Select"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Custom Select proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        refs,
        vec![
            "🌟 Singapore",
            "🌟 US",
            "🛬 Tokyo-A",
            "💎 高质量",
            "Tokyo-A-reality",
            "Tokyo-A-ss",
        ]
    );
}

#[test]
fn build_mihomo_yaml_remaps_landing_only_legacy_refs_before_helper_replay() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🛬 Legacy-A
    - 💎 高质量
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies:
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
      - 🛬 Legacy-A
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Custom Select"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Custom Select proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        refs,
        vec!["🌟 Singapore", "🌟 US", "🛬 Tokyo-A", "💎 高质量"]
    );
}

#[test]
fn build_mihomo_yaml_remaps_multiple_landing_only_legacy_refs_using_final_landing_order() {
    let u = user("u1", "alice");
    let nodes = vec![
        node("n1", "Tokyo B", "example.com"),
        node("n2", "Osaka A", "example.com"),
    ];
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_ss("e2", "n2", "ss", 8443, "BBBBBBBBBBBBBBBBBBBBBB=="),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n2", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group_with_relay:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 🛬 Legacy-B
    - 🛬 Legacy-A
    - 💎 高质量
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "Custom Select"
    type: select
    proxies:
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
      - 🛬 Legacy-B
      - 🛬 Legacy-A
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[
        ("n1", NodeSubscriptionRegion::Japan),
        ("n2", NodeSubscriptionRegion::Japan),
    ]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &nodes,
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Custom Select"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Custom Select proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        refs,
        vec![
            "🌟 Singapore",
            "🌟 US",
            "🛬 Osaka-A",
            "🛬 Tokyo-B",
            "💎 高质量",
        ]
    );
}

#[test]
fn build_mihomo_yaml_injects_default_high_quality_candidates() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules:\n  - MATCH,🚀 节点选择\n".to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile)
        .expect("build mihomo yaml should succeed");
    let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let group_refs = |name: &str| {
        v.get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("group proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        group_refs("🔒 高质量"),
        vec![
            "🌟 Japan",
            "🌟 HongKong",
            "🌟 Taiwan",
            "🌟 Korea",
            "🌟 Singapore",
            "🌟 US",
            "🌟 Other",
            "🛬 Tokyo-A",
            "Tokyo-A-reality",
        ]
    );
    assert_eq!(group_refs("💎 高质量"), vec!["🔒 高质量", "🤯 All"]);
}

#[test]
fn build_mihomo_yaml_preserves_existing_high_quality_provider_candidates() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🔒 高质量"
    type: url-test
    use: ["stale-provider"]
    filter: OldFilter
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: file
  path: ./provider-a.yaml
"#
        .to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile)
        .expect("build mihomo yaml should succeed");
    let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let high_quality = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 高质量"))
        })
        .expect("high quality group should exist");

    assert_eq!(
        high_quality.get("type").and_then(Value::as_str),
        Some("select")
    );
    assert_eq!(
        high_quality
            .get("use")
            .and_then(Value::as_sequence)
            .expect("current provider candidates should exist")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec!["providerA"]
    );
    assert_eq!(
        high_quality.get("filter").and_then(Value::as_str),
        Some("OldFilter")
    );
}

#[test]
fn build_mihomo_yaml_preserves_existing_canonical_star_region_order() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "Manual"
    type: select
    proxies: ["DIRECT"]
  - name: "Auto"
    type: select
    proxies: ["DIRECT", "🌟 US", "Manual", "🌟 Singapore"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let auto = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
        })
        .expect("Auto group should exist");
    let refs = auto
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("Auto proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(refs, vec!["DIRECT", "🌟 US", "Manual", "🌟 Singapore"]);
}

#[test]
fn build_mihomo_yaml_falls_back_to_in_place_order_when_specialized_helper_missing() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - 💎 高质量
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "💎 高质量"
    type: select
    proxies: ["DIRECT"]
  - name: "🗽 大流量"
    type: select
    proxies: ["DIRECT"]
  - name: "🎯 全球直连"
    type: select
    proxies: ["DIRECT"]
  - name: "🛑 全球拦截"
    type: select
    proxies: ["REJECT"]
  - name: "🐟 漏网之鱼"
    type: select
    proxies:
      - 🛑 全球拦截
      - 🗽 大流量
      - 🌟 US
      - 💎 高质量
      - 🛣️ JP/HK/TW
      - 🎯 全球直连
      - 🌟 Singapore
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("🐟 漏网之鱼"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("🐟 漏网之鱼 proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        refs,
        vec![
            "🛑 全球拦截",
            "🗽 大流量",
            "🌟 US",
            "💎 高质量",
            "🎯 全球直连",
            "🌟 Singapore",
        ]
    );
}

#[test]
fn build_mihomo_yaml_does_not_treat_extra_suffix_proxies_as_relay_groups() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
    - Manual
proxy-group_with_relay:
  proxies:
    - Manual
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "Manual"
    type: select
    proxies: ["DIRECT"]
  - name: "Auto"
    type: select
    proxies:
      - Tokyo-A-reality
      - Manual
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
      - 🌟 US
rules: []
"#
        .to_string(),
        extra_proxies_yaml: r#"
- name: Tokyo-A-reality
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Auto proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        refs,
        vec!["🌟 Singapore", "🌟 US", "Manual", "Tokyo-A-reality"]
    );
}

#[test]
fn build_mihomo_yaml_prunes_legacy_outer_ref_from_hidden_non_select_groups() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group:
  proxies:
    - 🌟 Japan
    - 🌟 Korea
    - 🌟 Singapore
    - 🌟 HongKong
    - 🌟 Taiwan
    - 🌟 US
port: 0
proxy-groups:
  - name: "🌟 Singapore"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "🌟 US"
    type: select
    hidden: true
    proxies: ["DIRECT"]
  - name: "Hidden Fallback"
    type: fallback
    hidden: true
    proxies:
      - 🌟 US
      - 🛣️ JP/HK/TW
      - 🌟 Singapore
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Hidden Fallback"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Hidden Fallback proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(refs, vec!["🌟 US", "🌟 Singapore"]);
    assert!(
        groups
            .iter()
            .all(|group| group.get("name").and_then(Value::as_str)
                != Some(MIHOMO_LEGACY_OUTER_GROUP))
    );
}

#[test]
fn build_mihomo_yaml_preserves_user_defined_relay_prefixed_groups() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ MyRelay"
    type: select
    proxies: ["DIRECT"]
  - name: "Auto"
    type: select
    proxies: ["🛣️ MyRelay", "DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    assert!(
        groups
            .iter()
            .any(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ MyRelay")),
        "custom relay-prefixed group should be preserved"
    );
    let auto_refs = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Auto proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(auto_refs, vec!["🛣️ MyRelay", "DIRECT"]);
}

#[test]
fn build_mihomo_yaml_preserves_standalone_user_defined_relay_prefixed_groups() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ MyRelay"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    assert!(
        groups
            .iter()
            .any(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ MyRelay")),
        "standalone custom relay-prefixed group should be preserved"
    );
}

#[test]
fn build_mihomo_yaml_preserves_mixin_defined_proxy_with_user_relay_dialer() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxies:
  - name: Custom-chain
    type: ss
    server: custom.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: "abc:def"
    udp: true
    dialer-proxy: 🛣️ MyRelay
proxy-groups:
  - name: "🛣️ MyRelay"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("🛣️ MyRelay")
    );
}

#[test]
fn build_mihomo_yaml_preserves_extra_relay_prefixed_proxy_dialer() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: 🛣️ MyRelayProxy
  type: ss
  server: relay.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ MyRelayProxy
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom extra proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("🛣️ MyRelayProxy")
    );
}

#[test]
fn build_mihomo_yaml_removes_user_defined_legacy_region_relay_prefixed_groups() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ Japan"
    type: select
    proxies: ["DIRECT"]
  - name: "Auto"
    type: select
    proxies: ["🛣️ Japan", "DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    assert!(
        groups
            .iter()
            .all(|group| group.get("name").and_then(Value::as_str) != Some("🛣️ Japan")),
        "legacy region relay-prefixed group should be removed"
    );
    let auto_refs = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Auto proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(auto_refs, vec!["DIRECT"]);
}

#[test]
fn build_mihomo_yaml_removes_custom_shared_outer_ref_and_group() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Hidden Fallback"
    type: fallback
    hidden: true
    proxies:
      - 🌟 US
      - 🛣️ JP/HK/SG
      - 🌟 Singapore
  - name: "🛣️ JP/HK/SG"
    type: url-test
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    let refs = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("Hidden Fallback"))
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Hidden Fallback proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(refs, vec!["🌟 US", "🌟 Singapore"]);
    assert!(groups.iter().all(|group| {
        group.get("name").and_then(Value::as_str) != Some(MIHOMO_SHARED_OUTER_GROUP)
    }));
}

#[test]
fn build_mihomo_yaml_removes_custom_shared_outer_group_referenced_by_rules() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ JP/HK/SG"
    type: url-test
    proxies: ["DIRECT"]
rules:
  - MATCH,🛣️ JP/HK/SG
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    assert!(
        groups.iter().all(|group| {
            group.get("name").and_then(Value::as_str) != Some(MIHOMO_SHARED_OUTER_GROUP)
        }),
        "legacy shared relay group should be removed even when rules target it"
    );
    assert_eq!(
        v.get("rules")
            .and_then(Value::as_sequence)
            .and_then(|rules| rules.first())
            .and_then(Value::as_str),
        Some("MATCH,DIRECT")
    );
}

#[test]
fn build_mihomo_yaml_remaps_legacy_relay_rule_target_without_custom_group() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
rules:
  - MATCH,🛣️ JP/HK/SG
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(
        v.get("rules")
            .and_then(Value::as_sequence)
            .and_then(|rules| rules.first())
            .and_then(Value::as_str),
        Some("MATCH,DIRECT")
    );
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    assert!(
        groups.iter().all(|group| {
            group.get("name").and_then(Value::as_str) != Some(MIHOMO_SHARED_OUTER_GROUP)
        }),
        "generated shared relay group should not return"
    );
}

#[test]
fn build_mihomo_yaml_preserves_unknown_relay_prefixed_rule_targets() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo-A", "new-host.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
rules:
  - DOMAIN,example.org,🛣️ old-host-example-com
  - DOMAIN,example.net,🛣️ new-host-example-com
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let rules = v
        .get("rules")
        .and_then(Value::as_sequence)
        .expect("rules must exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        rules,
        vec![
            "DOMAIN,example.org,🛣️ old-host-example-com",
            "DOMAIN,example.net,🛣️ new-host-example-com",
        ]
    );
}

#[test]
fn build_mihomo_yaml_maps_region_relay_ref_to_direct_in_extra_proxy_dialer_proxy() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo-A", "tokyo-a.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ Japan
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom extra proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("DIRECT")
    );
}

#[test]
fn build_mihomo_yaml_maps_shared_outer_ref_to_direct_in_extra_proxy_dialer_proxy() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo-A", "tokyo-a.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ JP/HK/SG
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom extra proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("DIRECT")
    );
}

#[test]
fn build_mihomo_yaml_removes_custom_shared_outer_dialer_proxy() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo-A", "tokyo-a.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ JP/HK/SG"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ JP/HK/SG
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    assert!(groups.iter().all(|group| {
        group.get("name").and_then(Value::as_str) != Some(MIHOMO_SHARED_OUTER_GROUP)
    }));
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom extra proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("DIRECT")
    );
}

#[test]
fn build_mihomo_yaml_maps_legacy_dialer_to_direct_without_relay_groups() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ JP/HK/SG
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom extra proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("DIRECT")
    );
}

#[test]
fn build_mihomo_yaml_removes_custom_region_relay_dialer_proxy() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo-A", "tokyo-a.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🛣️ Japan"
    type: select
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ Japan
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom extra proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("DIRECT")
    );
}

#[test]
fn build_mihomo_yaml_preserves_unknown_relay_prefixed_dialer_proxy() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ old-host
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom extra proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("🛣️ old-host")
    );
}

#[test]
fn build_mihomo_yaml_preserves_provider_backed_relay_prefixed_dialer_proxy() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: Custom-chain
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
  dialer-proxy: 🛣️ ProviderRelay
"#
        .to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/a
"#
        .to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let custom = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies
                .iter()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some("Custom-chain"))
        })
        .expect("custom extra proxy should exist");
    assert_eq!(
        custom.get("dialer-proxy").and_then(Value::as_str),
        Some("🛣️ ProviderRelay")
    );
}

#[test]
fn build_mihomo_yaml_rewrites_managed_us_region_refs_even_if_extra_proxy_shares_name() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Auto"
    type: select
    proxies: ["DIRECT", "🛣️ JP/HK/TW", "🔒 US", "Alpha-reality"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: r#"
- name: 🔒 US
  type: ss
  server: us.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "us:def"
  udp: true
- name: Alpha-reality
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();
    let auto = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
        })
        .expect("Auto group should exist");
    let refs = auto
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("Auto proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(refs, vec!["DIRECT", "🌟 US", "Alpha-reality"]);
}

#[test]
fn build_mihomo_yaml_does_not_generate_landing_groups_for_extra_proxies() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: "Legacy-ss"
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();

    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must be a sequence");
    assert!(
        !groups
            .iter()
            .any(|g| g.get("name").and_then(Value::as_str) == Some("🛬 Legacy")),
        "extra proxies must not create synthetic landing groups"
    );

    let landing = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("🔒 落地"))
        .expect("landing pool must exist");
    let landing_proxies = landing
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("landing proxies must exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(landing_proxies, vec!["DIRECT"]);

    let proxies = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("proxies must exist")
        .iter()
        .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(proxies, vec!["Legacy-ss"]);
}

#[test]
fn build_mihomo_yaml_keeps_region_labeled_extra_proxies_in_managed_groups() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: Singapore-A-reality
  type: ss
  server: sg.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
- name: Legacy-reality
  type: ss
  server: other.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");

    let singapore_refs = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 Singapore"))
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Singapore group proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(singapore_refs, vec!["Singapore-A-reality"]);

    let other_refs = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 Other"))
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("Other group proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(other_refs, vec!["Legacy-reality"]);
}

#[test]
fn build_mihomo_yaml_removes_template_landing_groups_when_base_missing() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Top"
    type: select
    proxies: ["🛬 Legacy"]
  - name: "🛬 Legacy"
    type: select
    proxies: ["SomeProxy"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();

    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must be a sequence");
    assert!(
        !groups
            .iter()
            .any(|g| g.get("name").and_then(Value::as_str) == Some("🛬 Legacy")),
        "expected template landing group 🛬 Legacy to be removed"
    );

    let top = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("Top"))
        .expect("Top group must exist");
    let top_proxies = top
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("Top proxies must exist");
    let top_proxy_names = top_proxies
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(top_proxy_names, vec!["DIRECT"]);
}

#[test]
fn build_mihomo_yaml_keeps_include_all_proxies_groups_without_direct_fallback() {
    let u = user("u1", "alice");
    let n = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Auto"
    type: url-test
    url: https://www.gstatic.com/generate_204
    interval: 10
    include-all-proxies: true
    filter: Tokyo
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n], &profile).unwrap();
    let v: Value = serde_yaml::from_str(&yaml).unwrap();

    let proxies = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("proxies must exist");
    assert!(!proxies.is_empty(), "generated proxies must be present");

    let groups = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");
    let auto = groups
        .iter()
        .find(|g| g.get("name").and_then(Value::as_str) == Some("Auto"))
        .expect("Auto group must exist");
    assert_eq!(
        auto.get("include-all-proxies"),
        Some(&Value::Bool(true)),
        "include-all-proxies must be preserved"
    );
    assert!(
        auto.get("proxies").is_none(),
        "DIRECT fallback should not be injected when include-all-proxies can supply candidates"
    );
}

#[test]
fn build_mihomo_yaml_injects_direct_when_relay_group_has_no_provider_candidates() {
    let u = user("u1", "alice");
    let n = node("n1", "Only US", "us.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0
rules: []
"
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Us)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &probes,
        &profile,
    )
    .expect("build mihomo yaml should succeed");
    let root: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups should exist");

    let relay = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ us-example-com"))
        .expect("relay group should exist");
    let proxies = relay
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("relay group should receive DIRECT fallback");
    assert_eq!(proxies, &vec![Value::String("DIRECT".to_string())]);
}

#[test]
fn build_mihomo_yaml_preserves_builtin_outbound_refs() {
    let u = user("u1", "alice");
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Auto"
    type: select
    proxies: ["DIRECT", "REJECT", "REJECT-DROP", "PASS", "COMPATIBLE"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &[], &[], &[], &profile)
        .expect("built-in outbound refs should be preserved");
    let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Auto"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("group proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        refs,
        vec!["DIRECT", "REJECT", "REJECT-DROP", "PASS", "COMPATIBLE"]
    );
}

#[test]
fn build_mihomo_yaml_remaps_supported_legacy_proxy_refs_and_prunes_old_region_refs() {
    let u = user("u1", "alice");
    let n1 = node("n1", "Alpha", "alpha.example.com");
    let n2 = node("n2", "Beta", "beta.example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
        endpoint_ss("e3", "n2", "ss", 443, "BBBBBBBBBBBBBBBBBBBBBB=="),
        endpoint_vless(
            "e4",
            "n2",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![
        membership("u1", "n1", "e1"),
        membership("u1", "n1", "e2"),
        membership("u1", "n2", "e3"),
        membership("u1", "n2", "e4"),
    ];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
helpers:
  keep: true
  proxies:
    - Legacy-A-reality
    - Legacy-B-JP
proxy-groups:
  - name: "🔒 高质量"
    type: select
    proxies:
      - Legacy-A-reality
      - Legacy-B-reality
      - Legacy-A-ss
      - Legacy-B-ss
      - Legacy-A-JP
      - Legacy-B-JP
      - Legacy-A-HK
      - Legacy-B-HK
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n1, n2], &profile)
        .expect("build mihomo yaml should succeed");
    let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");

    let group = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| groups.first())
        .expect("first group should exist");
    let refs = group
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("group proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        refs,
        vec![
            "🌟 Japan",
            "🌟 HongKong",
            "🌟 Taiwan",
            "🌟 Korea",
            "🌟 Singapore",
            "🌟 US",
            "🌟 Other",
            "🛬 Alpha",
            "🛬 Beta",
            "Alpha-reality",
            "Beta-reality",
        ]
    );

    let helper_refs = v
        .get("helpers")
        .and_then(Value::as_mapping)
        .and_then(|helpers| helpers.get(Value::String("proxies".to_string())))
        .and_then(Value::as_sequence)
        .expect("helper proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(helper_refs, vec!["Alpha-reality"]);
}

#[test]
fn build_mihomo_yaml_prunes_legacy_chain_refs_even_when_generated_count_is_smaller() {
    let u = user("u1", "alice");
    let n1 = node("n1", "Alpha", "alpha.example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
              "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🔒 高质量"
    type: select
    proxies:
      - Legacy-A-reality
      - Legacy-B-reality
      - Legacy-A-ss
      - Legacy-B-ss
      - Legacy-A-JP
      - Legacy-B-JP
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n1],
        &probes,
        &profile,
    )
    .expect("build mihomo yaml should succeed");
    let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 高质量"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("group proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        refs,
        vec![
            "🌟 Japan",
            "🌟 HongKong",
            "🌟 Taiwan",
            "🌟 Korea",
            "🌟 Singapore",
            "🌟 US",
            "🌟 Other",
            "🛬 Alpha",
            "Alpha-reality",
        ]
    );
}

#[test]
fn build_mihomo_yaml_preserves_extra_proxy_refs_with_chain_suffixes() {
    let u = user("u1", "alice");
    let n1 = node("n1", "Alpha", "alpha.example.com");
    let endpoints = vec![endpoint_ss(
        "e1",
        "n1",
        "ss",
        443,
        "AAAAAAAAAAAAAAAAAAAAAA==",
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "ExtraSelect"
    type: select
    proxies: ["Legacy-JP"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: r#"
- name: "Legacy-JP"
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n1],
        &probes,
        &profile,
    )
    .expect("build mihomo yaml should succeed");
    let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|g| g.get("name").and_then(Value::as_str) == Some("ExtraSelect"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("group proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(refs, vec!["Legacy-JP"]);
}

#[test]
fn build_mihomo_yaml_preserves_extra_proxy_refs_with_reality_suffixes() {
    let u = user("u1", "alice");
    let n1 = node("n1", "Alpha", "alpha.example.com");
    let endpoints = vec![endpoint_vless(
        "e1",
        "n1",
        "vless",
        8443,
        serde_json::json!({
          "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
          "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
          "short_ids": ["0123456789abcdef"],
          "active_short_id": "0123456789abcdef"
        }),
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "ExtraSelect"
    type: select
    proxies: ["Legacy-A-reality"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: r#"
- name: "Legacy-A-reality"
  type: ss
  server: extra.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#
        .to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n1],
        &probes,
        &profile,
    )
    .expect("build mihomo yaml should succeed");
    let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|g| g.get("name").and_then(Value::as_str) == Some("ExtraSelect"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("group proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(refs, vec!["Legacy-A-reality"]);

    let top_proxy_names = v
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("top-level proxies should exist")
        .iter()
        .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(top_proxy_names.contains(&"Alpha-reality"));
    assert!(top_proxy_names.contains(&"Legacy-A-reality"));
}

#[test]
fn build_mihomo_yaml_dedupes_all_proxy_refs_in_groups() {
    let u = user("u1", "alice");
    let n1 = node("n1", "Alpha", "alpha.example.com");
    let endpoints = vec![endpoint_vless(
        "e1",
        "n1",
        "vless",
        8443,
        serde_json::json!({
          "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
          "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
          "short_ids": ["0123456789abcdef"],
          "active_short_id": "0123456789abcdef"
        }),
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "Custom Select"
    type: select
    proxies:
      - 🛣️ JP/HK/TW
      - 🛣️ JP/HK/TW
      - Legacy-A-reality
      - Legacy-B-reality
  - name: 🛣️ JP/HK/TW
    type: select
    proxies: ["DIRECT"]
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: "".to_string(),
    };

    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n1],
        &probes,
        &profile,
    )
    .expect("build mihomo yaml should succeed");
    let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("Custom Select"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("group proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(refs, vec!["Alpha-reality"]);
}

#[test]
fn build_mihomo_yaml_flattens_and_removes_template_helper_reference_blocks() {
    let u = user("u1", "alice");
    let n1 = node("n1", "Alpha", "alpha.example.com");
    let endpoints = vec![endpoint_vless(
        "e1",
        "n1",
        "vless",
        8443,
        serde_json::json!({
          "reality": {"dest": "example.com:443", "server_names": ["sni.example.com"], "fingerprint": "chrome"},
          "reality_keys": {"private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "public_key": "PBK"},
          "short_ids": ["0123456789abcdef"],
          "active_short_id": "0123456789abcdef"
        }),
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
proxy-group:
  proxies: &subscription_proxies
    - Legacy-A-reality
proxy-use:
  use: &proxy_use
    - providerA
port: 0
proxy-groups:
  - name: "demo"
    type: select
    proxies: *subscription_proxies
    use: *proxy_use
rules: []
"#
        .to_string(),
        extra_proxies_yaml: "".to_string(),
        extra_proxy_providers_yaml: r#"
providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/sub-a
"#
        .to_string(),
    };

    let yaml = build_mihomo_yaml(SEED, &u, &memberships, &endpoints, &[n1], &profile)
        .expect("build mihomo yaml should succeed");
    let v: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");

    assert!(
        v.get("proxy-group").is_none() && v.get("proxy-use").is_none(),
        "helper reference blocks should be removed from final output"
    );

    let refs = v
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some("demo"))
        })
        .and_then(|group| group.get("proxies"))
        .and_then(Value::as_sequence)
        .expect("group proxies should exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(refs, vec!["Alpha-reality"]);
}
