use super::test_fixtures::{SEED, endpoint_vless, membership, node, probe_map, user, vless_meta};
use super::*;

use pretty_assertions::assert_eq;
use serde_yaml::Value;

fn group<'a>(root: &'a Value, name: &str) -> &'a Value {
    root.get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
        })
        .unwrap_or_else(|| panic!("missing {name} group"))
}

fn group_proxies(group: &Value) -> Vec<&str> {
    let proxies = group
        .get("proxies")
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    proxies.iter().filter_map(Value::as_str).collect()
}

#[test]
fn reality_direct_candidates_are_exposed_in_both_mihomo_routes() {
    let user = user("u1", "alice");
    let node = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![endpoint_vless(
        "e1",
        "n1",
        "vless",
        8443,
        vless_meta("example.com:443", &["sni.example.com"], true),
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
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
        extra_proxies_yaml: String::new(),
        extra_proxy_providers_yaml: String::new(),
    };
    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);

    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &user,
        &memberships,
        &endpoints,
        std::slice::from_ref(&node),
        &probes,
        &profile,
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(
        group_proxies(group(&root, "🚀 节点选择")),
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
            "💎 高质量",
        ]
    );
    assert_eq!(group_proxies(group(&root, "Custom Select")), vec!["DIRECT"]);

    let provider_yaml = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &user,
        &memberships,
        &endpoints,
        &[node],
        &probes,
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let provider_root: Value = serde_yaml::from_str(&provider_yaml).unwrap();
    let node_selector = group(&provider_root, "🚀 节点选择");
    assert_eq!(
        group_proxies(node_selector),
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
        node_selector
            .get("use")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec![MIHOMO_SYSTEM_PROVIDER_NAME]
    );
    assert_eq!(
        node_selector.get("filter").and_then(Value::as_str),
        Some("^Tokyo\\-A\\-reality$")
    );
    assert_eq!(
        group_proxies(group(&provider_root, "Custom Select")),
        vec!["DIRECT"]
    );
}

#[test]
fn landing_candidates_require_a_chain_and_keep_reality_first() {
    let direct_only = std::collections::BTreeSet::from(["Tokyo-A-reality".to_string()]);
    assert!(base_landing_proxy_names("Tokyo-A", &direct_only).is_empty());

    let proxy_names = std::collections::BTreeSet::from([
        "Tokyo-A-reality".to_string(),
        "Tokyo-A-ss".to_string(),
        "Tokyo-A-ss-chain".to_string(),
        "Tokyo-A-reality-chain".to_string(),
    ]);
    assert_eq!(
        base_landing_proxy_names("Tokyo-A", &proxy_names),
        vec![
            "Tokyo-A-reality".to_string(),
            "Tokyo-A-ss-chain".to_string(),
            "Tokyo-A-reality-chain".to_string(),
        ]
    );
}

#[test]
fn provider_reality_direct_names_follow_base_order() {
    let proxy_names =
        std::collections::BTreeSet::from(["a-2-reality".to_string(), "a-reality".to_string()]);

    assert_eq!(
        node_selector::provider_reality_access_names(&proxy_names),
        vec!["a-reality".to_string(), "a-2-reality".to_string()]
    );
}

#[test]
fn user_reality_named_proxy_is_not_injected_into_system_node_selector() {
    let user = user("u1", "alice");
    let node = node("n1", "Tokyo A", "example.com");
    let endpoints = vec![endpoint_vless(
        "e1",
        "n1",
        "vless",
        8443,
        vless_meta("example.com:443", &["sni.example.com"], true),
    )];
    let memberships = vec![membership("u1", "n1", "e1")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: r#"
- name: Personal-reality
  type: ss
  server: personal.example.invalid
  port: 443
  cipher: aes-128-gcm
  password: placeholder
"#
        .to_string(),
        extra_proxy_providers_yaml: String::new(),
    };
    let probes = probe_map(&[("n1", NodeSubscriptionRegion::Japan)]);

    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &user,
        &memberships,
        &endpoints,
        std::slice::from_ref(&node),
        &probes,
        &profile,
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    assert!(
        root["proxies"]
            .as_sequence()
            .unwrap()
            .iter()
            .any(|proxy| proxy["name"].as_str() == Some("Personal-reality"))
    );
    assert!(!group_proxies(group(&root, "🚀 节点选择")).contains(&"Personal-reality"));

    let provider_yaml = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &user,
        &memberships,
        &endpoints,
        &[node],
        &probes,
        &profile,
        "https://sub.example.com/api/sub/token/mihomo/provider/system",
    )
    .unwrap();
    let provider_root: Value = serde_yaml::from_str(&provider_yaml).unwrap();
    let node_selector = group(&provider_root, "🚀 节点选择");
    assert!(
        provider_root["proxies"]
            .as_sequence()
            .unwrap()
            .iter()
            .any(|proxy| proxy["name"].as_str() == Some("Personal-reality"))
    );
    assert!(
        !node_selector
            .get("filter")
            .and_then(Value::as_str)
            .unwrap()
            .contains("Personal\\-reality")
    );
}
