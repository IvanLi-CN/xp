use super::test_fixtures::*;
use super::*;

use pretty_assertions::assert_eq;
use serde_yaml::Value;
use std::collections::BTreeMap;
use xp_test_fixtures::{
    label_tokyo_a as fixture_label_tokyo_a, label_tokyo_b as fixture_label_tokyo_b,
    subscription_host_example as fixture_host_example, subscription_host_hkl as fixture_host_hkl,
    subscription_node_n1 as fixture_node_n1, subscription_node_n2 as fixture_node_n2,
};

#[test]
fn build_mihomo_yaml_expands_application_group_placeholders_in_both_routes() {
    let u = user("alice");
    let nodes = vec![
        node(
            fixture_node_n1(),
            fixture_label_tokyo_a,
            fixture_host_example(),
        ),
        node(fixture_node_n2(), fixture_label_tokyo_b, fixture_host_hkl()),
    ];
    let endpoints = vec![
        endpoint_vless("e2", "n1", "vless", 8443, VlessFixtureMode::Standard),
        endpoint_vless("e4", "n2", "vless", 9443, VlessFixtureMode::Standard),
    ];
    let memberships = vec![membership("n1", "e2"), membership("n2", "e4")];
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🎯 全球直连"
    type: select
    proxies: ["DIRECT"]
  - name: "🛑 全球拦截"
    type: select
    proxies: ["REJECT"]
  - name: "🎵 Spotify"
    type: select
    proxies:
      - "💎 节点选择"
      - "🌟 <region>"
      - "🛬 <node>"
      - "🎯 全球直连"
      - "🛑 全球拦截"
rules: []
"#
        .to_string(),
        extra_proxies_yaml: String::new(),
        extra_proxy_providers_yaml: String::new(),
    };
    let probes = probe_map(&[
        ("n1", NodeSubscriptionRegion::Japan),
        ("n2", NodeSubscriptionRegion::Singapore),
    ]);
    let expected_app = vec![
        "💎 节点选择".to_string(),
        "🌟 Japan".to_string(),
        "🌟 Singapore".to_string(),
        "🛬 Tokyo-A".to_string(),
        "🛬 Tokyo-B".to_string(),
        "🎯 全球直连".to_string(),
        "🛑 全球拦截".to_string(),
    ];

    let app_group_refs = |root: &Value| {
        root.get("proxy-groups")
            .and_then(Value::as_sequence)
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| group.get("name").and_then(Value::as_str) == Some("🎵 Spotify"))
            })
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("Spotify proxies should exist")
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let assert_system_contract = |root: &Value| {
        let groups = root
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .expect("proxy-groups should exist");
        assert!(!groups.iter().any(|group| {
            group
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| {
                    name.starts_with("🤯 ") && name != "🤯 All" && name != "🤯 落地"
                })
        }));
        let landing = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🔒 落地"))
            .expect("landing pool should exist");
        assert_eq!(
            landing.get("type").and_then(Value::as_str),
            Some("url-test")
        );
        let automatic = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("🤯 落地"))
            .expect("automatic landing pool should exist");
        assert_eq!(
            automatic.get("type").and_then(Value::as_str),
            Some("url-test")
        );
        assert_eq!(automatic.get("hidden").and_then(Value::as_bool), Some(true));
        for (group, expected_proxies) in [
            (landing, vec!["🛬 Tokyo-A", "🛬 Tokyo-B"]),
            (automatic, vec!["Tokyo-A-reality", "Tokyo-B-reality"]),
        ] {
            if let Some(proxies) = group.get("proxies").and_then(Value::as_sequence) {
                assert_eq!(
                    proxies.iter().filter_map(Value::as_str).collect::<Vec<_>>(),
                    expected_proxies
                );
            } else {
                let filter = group
                    .get("filter")
                    .and_then(Value::as_str)
                    .expect("provider-backed group should have an exact filter");
                assert!(filter.contains(r"Tokyo\-A\-reality"));
                assert!(filter.contains(r"Tokyo\-B\-reality"));
            }
        }
        let node_selector_fallback = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some("💎 节点选择"))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .expect("node selector fallback should expose proxies");
        assert_eq!(
            node_selector_fallback
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["🚀 节点选择", "🤯 落地", "🤯 All"]
        );
    };

    let direct_root: Value = serde_yaml::from_str(
        &build_mihomo_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &nodes,
            &probes,
            &profile,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(app_group_refs(&direct_root), expected_app);
    assert_system_contract(&direct_root);

    let provider_root: Value = serde_yaml::from_str(
        &build_mihomo_provider_yaml_with_node_probes(
            SEED,
            &u,
            &memberships,
            &endpoints,
            &nodes,
            &probes,
            &profile,
            xp_test_fixtures::subscription_provider_system_url(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(app_group_refs(&provider_root), expected_app);
    assert_system_contract(&provider_root);
}

#[test]
fn build_mihomo_provider_yaml_rejects_references_left_after_placeholder_expansion() {
    let profile = UserMihomoProfile {
        mixin_yaml: r#"
port: 0
proxy-groups:
  - name: "🎵 Spotify"
    type: select
    proxies:
      - "🌟 <region>"
      - "🛬 <node>"
      - "missing-group"
rules: []
"#
        .to_string(),
        extra_proxies_yaml: String::new(),
        extra_proxy_providers_yaml: String::new(),
    };

    let error = build_mihomo_provider_yaml_with_node_probes(
        SEED,
        &user("alice"),
        &[],
        &[],
        &[],
        &BTreeMap::new(),
        &profile,
        xp_test_fixtures::subscription_provider_system_url(),
    )
    .expect_err("undefined references must fail after placeholder expansion");

    assert!(matches!(
        error,
        SubscriptionError::MihomoInvalidFinalConfigReference { target, .. }
            if target == "missing-group"
    ));
}

#[test]
fn mihomo_dynamic_region_placeholders_ignore_chain_only_candidates() {
    let proxy_names = std::collections::BTreeSet::from(["Tokyo-A-ss-chain".to_string()]);
    let region_map =
        std::collections::BTreeMap::from([("Tokyo-A".to_string(), NodeSubscriptionRegion::Japan)]);

    assert!(mihomo_dynamic_region_group_names(&proxy_names, &region_map).is_empty());
}
