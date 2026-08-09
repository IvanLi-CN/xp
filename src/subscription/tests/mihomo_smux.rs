use super::*;

use pretty_assertions::assert_eq;
use serde_yaml::Value;

fn assert_default_smux(proxy: &Value) {
    let smux = proxy.get("smux").expect("generated proxy must use SMux");
    assert_eq!(smux.get("enabled"), Some(&Value::Bool(true)));
    assert_eq!(
        smux.get("protocol"),
        Some(&Value::String("smux".to_string()))
    );
    assert_eq!(
        smux.get("max-connections"),
        Some(&Value::Number(4_u64.into()))
    );
    assert_eq!(smux.get("min-streams"), Some(&Value::Number(4_u64.into())));
    assert_eq!(smux.get("padding"), Some(&Value::Bool(false)));
    assert_eq!(smux.get("statistic"), Some(&Value::Bool(false)));
    assert_eq!(smux.get("only-tcp"), Some(&Value::Bool(true)));
}

#[test]
fn build_clash_yaml_has_proxies_and_derived_secrets() {
    let u = user("u1", "alice");
    let n = node("n1", xp_test_fixtures::slot_s675, "example.com");
    let endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            serde_json::json!({
              "reality": {
                "dest": "example.com:443",
                "server_names": ["sni.example.com"],
                "fingerprint": "chrome"
              },
              "reality_keys": {
                "private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "public_key": "PBK"
              },
              "short_ids": ["0123456789abcdef"],
              "active_short_id": "0123456789abcdef"
            }),
        ),
    ];
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let yaml = build_clash_yaml(SEED, &u, &memberships, &endpoints, &[n]).unwrap();
    let value: Value = serde_yaml::from_str(&yaml).unwrap();
    let proxies = value
        .get("proxies")
        .and_then(Value::as_sequence)
        .expect("proxies must be a list");
    assert_eq!(proxies.len(), 2);

    let ss = proxies
        .iter()
        .find(|proxy| proxy.get("type") == Some(&Value::String("ss".to_string())))
        .unwrap();
    assert_eq!(
        ss.get("server"),
        Some(&Value::String(
            xp_test_fixtures::subscription_host_example().to_owned(),
        ))
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
    assert_default_smux(ss);

    let vless = proxies
        .iter()
        .find(|proxy| proxy.get("type") == Some(&Value::String("vless".to_string())))
        .unwrap();
    assert_eq!(
        vless.get("server"),
        Some(&Value::String(
            xp_test_fixtures::subscription_host_example().to_owned(),
        ))
    );
    assert_eq!(vless.get("port"), Some(&Value::Number(8443.into())));
    let expected_uuid =
        crate::credentials::derive_vless_uuid(SEED, "u1", u.credential_epoch).unwrap();
    assert_eq!(vless.get("uuid"), Some(&Value::String(expected_uuid)));
    assert!(vless.get("smux").is_none());
}

#[test]
fn mihomo_system_payload_limits_endpoint_smux_to_ss2022_and_keeps_raw_uris_standard() {
    let u = user("u1", "alice");
    let n = node("n1", xp_test_fixtures::slot_s675, "example.com");
    let mut endpoints = vec![
        endpoint_ss("e1", "n1", "ss", 443, "AAAAAAAAAAAAAAAAAAAAAA=="),
        endpoint_vless(
            "e2",
            "n1",
            "vless",
            8443,
            vless_meta("example.com:443", &["sni.example.com"], false),
        ),
    ];
    endpoints[1].meta["mihomo_smux"] = serde_json::json!({
        "enabled": true,
        "max_connections": 4,
        "min_streams": 4,
        "only_tcp": true
    });
    let memberships = vec![membership("u1", "n1", "e1"), membership("u1", "n1", "e2")];
    let raw_before =
        build_raw_text(SEED, &u, &memberships, &endpoints, std::slice::from_ref(&n)).unwrap();
    let base64_before =
        build_base64(SEED, &u, &memberships, &endpoints, std::slice::from_ref(&n)).unwrap();
    assert!(!raw_before.contains("smux"));

    let yaml = build_mihomo_provider_system_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        std::slice::from_ref(&n),
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let proxies = root["proxies"].as_sequence().unwrap();
    assert_eq!(proxies.len(), 4);
    for proxy in proxies {
        if proxy["type"].as_str() == Some("ss") {
            assert_default_smux(proxy);
        } else {
            assert!(proxy.get("smux").is_none());
        }
    }
    let direct_vless = proxies
        .iter()
        .find(|proxy| proxy["name"].as_str() == Some("node-1-reality"))
        .expect("system provider must retain the direct VLESS entry");
    assert_eq!(direct_vless["type"].as_str(), Some("vless"));
    assert!(direct_vless.get("dialer-proxy").is_none());
    assert!(direct_vless.get("smux").is_none());

    let chained_vless = proxies
        .iter()
        .find(|proxy| proxy["name"].as_str() == Some("node-1-reality-chain"))
        .expect("system provider must retain the chained VLESS entry");
    assert_eq!(chained_vless["type"].as_str(), Some("vless"));
    assert_eq!(
        chained_vless["dialer-proxy"].as_str(),
        Some("🛣️ example-com")
    );
    assert!(chained_vless.get("smux").is_none());

    endpoints[0].meta["mihomo_smux"] = serde_json::json!({
        "enabled": false,
        "max_connections": 4,
        "min_streams": 4,
        "only_tcp": true
    });
    let raw_after =
        build_raw_text(SEED, &u, &memberships, &endpoints, std::slice::from_ref(&n)).unwrap();
    assert_eq!(raw_after, raw_before);
    let base64_after =
        build_base64(SEED, &u, &memberships, &endpoints, std::slice::from_ref(&n)).unwrap();
    assert_eq!(base64_after, base64_before);

    let yaml = build_mihomo_provider_system_yaml(
        SEED,
        &u,
        &memberships,
        &endpoints,
        std::slice::from_ref(&n),
    )
    .unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    for proxy in root["proxies"].as_sequence().unwrap() {
        let name = proxy["name"].as_str().unwrap();
        if name.contains("-ss") {
            assert!(proxy.get("smux").is_none());
        } else {
            assert!(proxy.get("smux").is_none());
        }
    }
}
