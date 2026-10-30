use super::*;

use pretty_assertions::assert_eq;
use serde_yaml::Value;

fn xhttp_endpoint() -> Endpoint {
    let mut endpoint = endpoint_vless("e1", "n1", "vless", 8443, VlessFixtureMode::Standard);
    endpoint.meta["transport"] = serde_json::json!("xhttp");
    endpoint
}

fn profile() -> UserMihomoProfile {
    UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: String::new(),
        extra_proxy_providers_yaml: String::new(),
    }
}

fn proxies(root: &Value) -> &[Value] {
    root.get("proxies")
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
        .expect("proxies must be a sequence")
}

fn named_proxy<'a>(root: &'a Value, name: &str) -> &'a Value {
    proxies(root)
        .iter()
        .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some(name))
        .unwrap_or_else(|| panic!("missing proxy {name}"))
}

fn assert_xhttp_proxy(proxy: &Value) {
    assert_eq!(proxy.get("network").and_then(Value::as_str), Some("xhttp"));
    assert!(
        proxy
            .get("flow")
            .is_none_or(|flow| flow.as_str() == Some(""))
    );
    assert_eq!(
        proxy
            .get("alpn")
            .and_then(Value::as_sequence)
            .expect("xhttp alpn")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec!["h2"]
    );

    let xhttp = proxy
        .get("xhttp-opts")
        .and_then(Value::as_mapping)
        .expect("xhttp options");
    assert_eq!(
        xhttp.get("path").and_then(Value::as_str),
        Some(crate::protocol::VLESS_XHTTP_PATH)
    );
    assert_eq!(
        xhttp.get("mode").and_then(Value::as_str),
        Some("stream-one")
    );

    let reuse = xhttp
        .get("reuse-settings")
        .and_then(Value::as_mapping)
        .expect("xhttp reuse settings");
    for (key, expected) in [
        ("max-connections", "1"),
        ("max-concurrency", "0"),
        ("c-max-reuse-times", "0"),
        ("h-max-request-times", "0"),
        ("h-max-reusable-secs", "0"),
    ] {
        assert_eq!(
            reuse.get(key).and_then(Value::as_str),
            Some(expected),
            "unexpected {key}"
        );
    }
    assert_eq!(
        reuse.get("h-keep-alive-period").and_then(Value::as_i64),
        Some(-1)
    );
}

#[test]
fn legacy_vless_defaults_to_byte_stable_vision_tcp_output() {
    let u = user("alice");
    let n = node(
        fixture_node_n1(),
        fixture_label_node1_variant2,
        fixture_host_example(),
    );
    let legacy = endpoint_vless("e1", "n1", "vless", 8443, VlessFixtureMode::Standard);
    assert!(legacy.meta.get("transport").is_none());
    let mut explicit = legacy.clone();
    explicit.meta["transport"] = serde_json::json!("vision_tcp");
    let membership = membership("n1", "e1");

    let legacy_raw = build_raw_text(
        SEED,
        &u,
        std::slice::from_ref(&membership),
        std::slice::from_ref(&legacy),
        std::slice::from_ref(&n),
    )
    .unwrap();
    let explicit_raw = build_raw_text(
        SEED,
        &u,
        std::slice::from_ref(&membership),
        std::slice::from_ref(&explicit),
        std::slice::from_ref(&n),
    )
    .unwrap();
    assert_eq!(legacy_raw, explicit_raw);

    let uuid = crate::credentials::derive_vless_uuid(SEED, "u1", 0).unwrap();
    assert_eq!(
        legacy_raw,
        format!(
            concat!(
                "vless://{}@{}:8443?encryption=none&security=reality&type=tcp",
                "&sni=host-35.fixture.test&fp=chrome&pbk=&sid=0123456789abcdef",
                "&flow=xtls-rprx-vision#alice-node-1-vless\n"
            ),
            uuid,
            fixture_host_example()
        )
    );

    let legacy_yaml = build_clash_yaml(
        SEED,
        &u,
        std::slice::from_ref(&membership),
        std::slice::from_ref(&legacy),
        std::slice::from_ref(&n),
    )
    .unwrap();
    let explicit_yaml = build_clash_yaml(SEED, &u, &[membership], &[explicit], &[n]).unwrap();
    assert_eq!(legacy_yaml, explicit_yaml);
    let root: Value = serde_yaml::from_str(&legacy_yaml).unwrap();
    let proxy = &proxies(&root)[0];
    assert_eq!(proxy.get("network").and_then(Value::as_str), Some("tcp"));
    assert_eq!(
        proxy.get("flow").and_then(Value::as_str),
        Some("xtls-rprx-vision")
    );
    assert!(proxy.get("alpn").is_none());
    assert!(proxy.get("xhttp-opts").is_none());
}

#[test]
fn xhttp_clash_yaml_and_raw_uri_carry_the_fixed_reuse_contract() {
    let u = user("alice");
    let n = node(
        fixture_node_n1(),
        fixture_label_node1_variant2,
        fixture_host_example(),
    );
    let endpoint = xhttp_endpoint();
    let membership = membership("n1", "e1");

    let yaml = build_clash_yaml(
        SEED,
        &u,
        std::slice::from_ref(&membership),
        std::slice::from_ref(&endpoint),
        std::slice::from_ref(&n),
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    assert_xhttp_proxy(&proxies(&root)[0]);

    let raw = build_raw_text(SEED, &u, &[membership], &[endpoint], &[n]).unwrap();
    let uri = reqwest::Url::parse(raw.trim()).expect("valid VLESS URI");
    let query = uri
        .query_pairs()
        .into_owned()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(query.get("type").map(String::as_str), Some("xhttp"));
    assert_eq!(query.get("alpn").map(String::as_str), Some("h2"));
    assert_eq!(
        query.get("path").map(String::as_str),
        Some(crate::protocol::VLESS_XHTTP_PATH)
    );
    assert_eq!(query.get("mode").map(String::as_str), Some("stream-one"));
    assert!(!query.contains_key("flow"));

    let extra: serde_json::Value = serde_json::from_str(&query["extra"]).unwrap();
    assert_eq!(extra["xmux"]["maxConnections"], "1");
    assert_eq!(extra["xmux"]["maxConcurrency"], "0");
    assert_eq!(extra["xmux"]["cMaxReuseTimes"], "0");
    assert_eq!(extra["xmux"]["hMaxRequestTimes"], "0");
    assert_eq!(extra["xmux"]["hMaxReusableSecs"], "0");
    assert_eq!(extra["xmux"]["hKeepAlivePeriod"], -1);
}

#[test]
fn custom_xmux_is_used_in_yaml_and_raw_uri() {
    let u = user("alice");
    let n = node(
        fixture_node_n1(),
        fixture_label_node1_variant2,
        fixture_host_example(),
    );
    let mut endpoint = xhttp_endpoint();
    endpoint.meta["mihomo_xmux"] = serde_json::json!({
        "max_connections": "0",
        "max_concurrency": "16-32",
        "c_max_reuse_times": "0-20",
        "h_max_reusable_secs": "0-120",
        "h_keep_alive_period": 45
    });
    let membership = membership("n1", "e1");
    let yaml = build_clash_yaml(
        SEED,
        &u,
        std::slice::from_ref(&membership),
        std::slice::from_ref(&endpoint),
        std::slice::from_ref(&n),
    )
    .unwrap();
    if let Some(binary) = std::env::var_os("XP_TEST_MIHOMO_BIN") {
        let fixture = tempfile::NamedTempFile::new().unwrap();
        let mut connection_endpoint = endpoint.clone();
        connection_endpoint.meta["mihomo_xmux"]["max_connections"] = serde_json::json!("2-4");
        connection_endpoint.meta["mihomo_xmux"]["max_concurrency"] = serde_json::json!("0");
        let connection_yaml = build_clash_yaml(
            SEED,
            &u,
            std::slice::from_ref(&membership),
            &[connection_endpoint],
            std::slice::from_ref(&n),
        )
        .unwrap();
        for generated in [&yaml, &connection_yaml] {
            std::fs::write(fixture.path(), generated).unwrap();
            let result = std::process::Command::new(&binary)
                .args(["-t", "-f"])
                .arg(fixture.path())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "Mihomo rejected generated YAML: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let reuse = &proxies(&root)[0]["xhttp-opts"]["reuse-settings"];
    assert_eq!(reuse["max-concurrency"].as_str(), Some("16-32"));
    assert_eq!(reuse["c-max-reuse-times"].as_str(), Some("0-20"));
    assert_eq!(reuse["h-max-reusable-secs"].as_str(), Some("0-120"));
    assert_eq!(reuse["h-keep-alive-period"].as_i64(), Some(45));
    let profile = profile();
    let main_yaml = build_mihomo_yaml(
        SEED,
        &u,
        std::slice::from_ref(&membership),
        std::slice::from_ref(&endpoint),
        std::slice::from_ref(&n),
        &profile,
    )
    .unwrap();
    let main: Value = serde_yaml::from_str(&main_yaml).unwrap();
    let system_yaml = build_mihomo_provider_system_yaml(
        SEED,
        &u,
        std::slice::from_ref(&membership),
        std::slice::from_ref(&endpoint),
        std::slice::from_ref(&n),
    )
    .unwrap();
    let system: Value = serde_yaml::from_str(&system_yaml).unwrap();
    for rendered in [&main, &system] {
        for name in ["node-1-reality", "node-1-reality-chain"] {
            assert_eq!(
                &named_proxy(rendered, name)["xhttp-opts"]["reuse-settings"],
                reuse
            );
        }
    }
    let raw = build_raw_text(SEED, &u, &[membership], &[endpoint], &[n]).unwrap();
    let url = reqwest::Url::parse(raw.trim()).unwrap();
    let extra = url.query_pairs().find(|(key, _)| key == "extra").unwrap().1;
    let extra: serde_json::Value = serde_json::from_str(&extra).unwrap();
    assert_eq!(extra["xmux"]["maxConcurrency"], "16-32");
    assert_eq!(extra["xmux"]["hKeepAlivePeriod"], 45);
}

#[test]
fn xhttp_is_preserved_for_direct_chain_and_provider_render_paths() {
    let u = user("alice");
    let n = node(
        fixture_node_n1(),
        fixture_label_node1_variant2,
        fixture_host_example(),
    );
    let endpoint = xhttp_endpoint();
    let membership = membership("n1", "e1");
    let profile = profile();

    let yaml = build_mihomo_yaml(
        SEED,
        &u,
        std::slice::from_ref(&membership),
        std::slice::from_ref(&endpoint),
        std::slice::from_ref(&n),
        &profile,
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let direct = named_proxy(&root, "node-1-reality");
    assert_xhttp_proxy(direct);
    assert!(direct.get("dialer-proxy").is_none());
    let chain = named_proxy(&root, "node-1-reality-chain");
    assert_xhttp_proxy(chain);
    assert!(chain.get("dialer-proxy").is_some());

    let primary_yaml = build_mihomo_provider_yaml(
        SEED,
        &u,
        std::slice::from_ref(&membership),
        std::slice::from_ref(&endpoint),
        std::slice::from_ref(&n),
        &profile,
        xp_test_fixtures::subscription_provider_system_url(),
    )
    .unwrap();
    let primary: Value = serde_yaml::from_str(&primary_yaml).unwrap();
    assert!(
        primary
            .get("proxy-providers")
            .and_then(Value::as_mapping)
            .is_some_and(|providers| providers.contains_key(MIHOMO_SYSTEM_PROVIDER_NAME))
    );

    let system_yaml =
        build_mihomo_provider_system_yaml(SEED, &u, &[membership], &[endpoint], &[n]).unwrap();
    let system: Value = serde_yaml::from_str(&system_yaml).unwrap();
    assert_xhttp_proxy(named_proxy(&system, "node-1-reality"));
    assert_xhttp_proxy(named_proxy(&system, "node-1-reality-chain"));
}
