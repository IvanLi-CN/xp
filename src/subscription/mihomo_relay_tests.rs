use super::test_fixtures::*;
use super::*;

use serde_yaml::Value;
use xp_test_fixtures::{
    endpoint_server_psk_b64, label_osaka_b as fixture_label_osaka_b,
    label_seoul_a as fixture_label_seoul_a, label_tokyo_a as fixture_label_tokyo_a,
    label_tokyo_b as fixture_label_tokyo_b, subscription_api_seoul_a as fixture_api_seoul_a,
    subscription_api_tokyo_a as fixture_api_tokyo_a,
    subscription_api_tokyo_b as fixture_api_tokyo_b,
    subscription_host_example as fixture_host_example,
    subscription_host_relay_a as fixture_host_relay_a,
    subscription_host_relay_b as fixture_host_relay_b,
    subscription_host_seoul as fixture_host_seoul, subscription_host_shared as fixture_host_shared,
    subscription_node_n1 as fixture_node_n1, subscription_node_n2 as fixture_node_n2,
    subscription_node_n3 as fixture_node_n3,
};

#[test]
fn build_mihomo_provider_yaml_groups_relay_by_access_host() {
    let u = user("alice");
    let n1 = node_with_api_base(
        fixture_node_n1(),
        fixture_label_tokyo_a,
        fixture_host_shared(),
        fixture_api_tokyo_a(),
    );
    let n2 = node_with_api_base(
        fixture_node_n2(),
        fixture_label_tokyo_b,
        fixture_host_shared(),
        fixture_api_tokyo_b(),
    );
    let n3 = node_with_api_base(
        fixture_node_n3(),
        fixture_label_seoul_a,
        fixture_host_seoul(),
        fixture_api_seoul_a(),
    );
    let endpoints = vec![
        endpoint_ss("e3", "n3", "ss", 443, endpoint_server_psk_b64()),
        endpoint_vless("e1", "n1", "vless", 8443, VlessFixtureMode::Standard),
        endpoint_vless("e4", "n2", "vless", 8443, VlessFixtureMode::Standard),
    ];
    let memberships = vec![
        membership("n3", "e3"),
        membership("n1", "e1"),
        membership("n2", "e4"),
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
        xp_test_fixtures::subscription_provider_system_url(),
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
        vec!["🛣️ seoul-fixture-test", "🛣️ shared-fixture-test"]
    );
    let relay_url = |name: &str| {
        groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|group| group.get("url"))
            .and_then(Value::as_str)
    };
    assert_eq!(
        relay_url("🛣️ shared-fixture-test"),
        Some(MIHOMO_DEFAULT_HEALTH_CHECK_URL)
    );
    assert_eq!(
        relay_url("🛣️ seoul-fixture-test"),
        Some(xp_test_fixtures::subscription_health_seoul_a())
    );
    let relay_candidates = |name: &str| {
        groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|group| group.get("filter"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let shared_relay = groups
        .iter()
        .find(|group| group.get("name").and_then(Value::as_str) == Some("🛣️ shared-fixture-test"))
        .expect("shared relay should exist");
    assert_eq!(
        shared_relay
            .get("proxies")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec![MIHOMO_RELAY_REJECT_FALLBACK]
    );
    assert!(shared_relay.get("use").is_none());
    assert!(shared_relay.get("filter").is_none());
    let seoul_filter = relay_candidates("🛣️ seoul-fixture-test")
        .expect("seoul relay should filter system and external candidates");
    assert!(
        seoul_filter.contains("Tokyo\\-A\\-reality"),
        "unexpected seoul filter: {seoul_filter}"
    );
    assert!(
        seoul_filter.contains("Tokyo\\-B\\-reality"),
        "unexpected seoul filter: {seoul_filter}"
    );
    assert!(!seoul_filter.contains("Seoul\\-A\\-reality"));
    for relay_name in ["🛣️ seoul-fixture-test"] {
        let use_values = groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some(relay_name))
            .and_then(|group| group.get("use"))
            .and_then(Value::as_sequence)
            .expect("relay should use the system and external providers")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(use_values, vec![MIHOMO_SYSTEM_PROVIDER_NAME, "providerA"]);
    }

    let system_yaml = build_mihomo_provider_system_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[
            node_with_api_base(
                fixture_node_n1(),
                fixture_label_tokyo_a,
                fixture_host_shared(),
                fixture_api_tokyo_a(),
            ),
            node_with_api_base(
                fixture_node_n2(),
                fixture_label_tokyo_b,
                fixture_host_shared(),
                fixture_api_tokyo_b(),
            ),
            node_with_api_base(
                fixture_node_n3(),
                fixture_label_seoul_a,
                fixture_host_seoul(),
                fixture_api_seoul_a(),
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
        proxy_dialer("Tokyo-A-reality-chain").as_deref(),
        Some("🛣️ shared-fixture-test")
    );
    assert_eq!(
        proxy_dialer("Tokyo-B-reality-chain").as_deref(),
        Some("🛣️ shared-fixture-test")
    );
    assert_eq!(
        proxy_dialer("Seoul-A-ss-chain").as_deref(),
        Some("🛣️ seoul-fixture-test")
    );
}

#[test]
fn build_mihomo_provider_yaml_relay_rejects_when_no_other_reality_exists() {
    let u = user("alice");
    let n = node(
        fixture_node_n1(),
        fixture_label_tokyo_a,
        fixture_host_example(),
    );
    let endpoints = vec![endpoint_vless(
        "e1",
        "n1",
        "vless",
        8443,
        VlessFixtureMode::Standard,
    )];
    let memberships = vec![membership("n1", "e1")];
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
        xp_test_fixtures::subscription_provider_system_url(),
    )
    .expect("provider YAML should build");
    let root: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let relay = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups.iter().find(|group| {
                group.get("name").and_then(Value::as_str) == Some("🛣️ example-fixture-test")
            })
        })
        .expect("relay group should exist");
    assert_eq!(
        relay
            .get("proxies")
            .and_then(Value::as_sequence)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec![MIHOMO_RELAY_REJECT_FALLBACK]
    );
    assert_eq!(
        relay.get("empty-fallback"),
        Some(&Value::String(MIHOMO_RELAY_REJECT_FALLBACK.to_string()))
    );
    assert!(relay.get("use").is_none());
    assert!(relay.get("filter").is_none());
}

#[test]
fn build_mihomo_yaml_relay_uses_other_subscribed_reality_only() {
    let u = user("alice");
    let n1 = node(
        fixture_node_n1(),
        fixture_label_tokyo_a,
        fixture_host_relay_a(),
    );
    let n2 = node(
        fixture_node_n2(),
        fixture_label_osaka_b,
        fixture_host_relay_b(),
    );
    let n3 = node(
        fixture_node_n3(),
        fixture_label_seoul_a,
        fixture_host_seoul(),
    );
    let endpoints = vec![
        endpoint_vless("e1", "n1", "vless", 8443, VlessFixtureMode::Standard),
        endpoint_vless("e4", "n2", "vless", 9443, VlessFixtureMode::Standard),
    ];
    let memberships = vec![membership("n1", "e1"), membership("n2", "e4")];
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
    let yaml = build_mihomo_yaml_with_node_probes(
        SEED,
        &u,
        &memberships,
        &endpoints,
        &[n1, n2, n3],
        &probes,
        &profile,
    )
    .expect("build Mihomo YAML should succeed");
    let root: Value = serde_yaml::from_str(&yaml).expect("result should be valid yaml");
    let groups = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .expect("proxy-groups should exist");

    let relay = |name: &str| {
        groups
            .iter()
            .find(|group| group.get("name").and_then(Value::as_str) == Some(name))
            .expect("relay group should exist")
    };
    let candidates = |name: &str| {
        relay(name)
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("relay should include static Reality candidates")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        candidates("🛣️ relay-dash-a-fixture-test"),
        vec!["Osaka-B-reality"]
    );
    assert_eq!(
        candidates("🛣️ relay-dash-b-fixture-test"),
        vec!["Tokyo-A-reality"]
    );
    for relay_name in [
        "🛣️ relay-dash-a-fixture-test",
        "🛣️ relay-dash-b-fixture-test",
    ] {
        let group = relay(relay_name);
        assert_eq!(
            group
                .get("use")
                .and_then(Value::as_sequence)
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["providerA"]
        );
        assert_eq!(
            group.get("filter").and_then(Value::as_str),
            Some(MIHOMO_OUTER_FILTER)
        );
    }
    let all_group = relay("🤯 All");
    let all_refs = all_group
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("All group should expose candidates")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        all_refs,
        vec![
            "🛬 Osaka-B",
            "🛬 Tokyo-A",
            "🤯 Japan",
            "🤯 HongKong",
            "🤯 Taiwan",
            "🤯 Korea",
            "🤯 Singapore",
            "🤯 US",
            "🤯 Other",
        ]
    );
}
