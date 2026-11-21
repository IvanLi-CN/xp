use super::test_fixtures::*;
use super::*;

use serde_yaml::Value;
use xp_test_fixtures::{
    endpoint_server_psk_b64, label_tokyo_a as fixture_label_tokyo_a,
    subscription_host_example as fixture_host_example, subscription_node_n1 as fixture_node_n1,
};

#[test]
fn provider_groups_fail_closed_and_keep_the_direct_escape() {
    let u = user("alice");
    let n = node(
        fixture_node_n1(),
        fixture_label_tokyo_a,
        fixture_host_example(),
    );
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, endpoint_server_psk_b64()),
        endpoint_vless("e2", "n1", "vless", 8443, VlessFixtureMode::Standard),
    ];
    let memberships = vec![membership("n1", "e1"), membership("n1", "e2")];
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: String::new(),
        extra_proxy_providers_yaml: String::new(),
    };

    let yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n],
        &profile,
        xp_test_fixtures::subscription_provider_system_url(),
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups must exist");

    let mut provider_derived = Vec::<&str>::new();
    for group in groups {
        let has_provider_candidates = group
            .get("use")
            .and_then(Value::as_sequence)
            .map(|seq| !seq.is_empty())
            .unwrap_or(false);
        if !has_provider_candidates || group.get("proxies").is_some() {
            continue;
        }
        let name = group
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unnamed>");
        provider_derived.push(name);
        assert_eq!(
            group.get("empty-fallback").and_then(Value::as_str),
            Some(MIHOMO_FAIL_CLOSED_FALLBACK),
            "provider-derived group {name} must declare an explicit empty fallback"
        );
    }

    assert!(
        provider_derived.contains(&"🛬 Tokyo-A"),
        "landing group must be provider-derived"
    );
    assert!(
        provider_derived.contains(&"🔒 Japan"),
        "visible region group must be provider-derived"
    );
    assert!(
        provider_derived.contains(&MIHOMO_AUTOMATIC_LANDING_POOL_GROUP),
        "automatic landing pool must be provider-derived"
    );

    for aggregate in ["💎 高质量", "💎 节点选择"] {
        let escape = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some(aggregate))
            .and_then(|group| group.get("proxies"))
            .and_then(Value::as_sequence)
            .and_then(|seq| seq.last())
            .and_then(Value::as_str);
        assert_eq!(
            escape,
            Some("DIRECT"),
            "aggregate {aggregate} must end with the trailing DIRECT escape member"
        );
    }
}
