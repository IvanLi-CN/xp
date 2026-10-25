use super::*;

#[test]
fn preserves_vless_encryption_and_explicit_empty_reality_short_id() {
    let url = Url::parse(concat!(
        "vless://00000000-0000-0000-0000-000000000001@example.com:443?",
        "encryption=mlkem768x25519plus.native%2Fxorpub%2Ftoken&",
        "security=reality&pbk=public-key&sid=#reality"
    ))
    .expect("URL");
    let (proxy, _, _, _) = parse_vless(url, Some("reality".to_owned())).expect("proxy");
    assert_eq!(
        proxy["encryption"].as_str(),
        Some("mlkem768x25519plus.native/xorpub/token")
    );
    assert_eq!(proxy["reality-opts"]["short-id"].as_str(), Some(""));
}

#[test]
fn defaults_missing_reality_short_id_to_empty() {
    let url = Url::parse(concat!(
        "vless://00000000-0000-0000-0000-000000000001@example.com:443?",
        "security=reality&pbk=public-key#reality"
    ))
    .expect("URL");
    let (proxy, _, _, _) = parse_vless(url, None).expect("proxy");
    assert_eq!(proxy["reality-opts"]["short-id"].as_str(), Some(""));
}

#[test]
fn preserves_vless_tcp_http_disguise() {
    let url = Url::parse(concat!(
        "vless://00000000-0000-0000-0000-000000000001@example.com:443?",
        "type=tcp&headerType=http&path=%2Fvideo&host=cdn.example%2Ccdn-2.example"
    ))
    .expect("URL");
    let (proxy, _, _, _) = parse_vless(url, None).expect("proxy");
    assert_eq!(proxy["network"].as_str(), Some("http"));
    assert_eq!(proxy["http-opts"]["path"][0].as_str(), Some("/video"));
    assert_eq!(
        proxy["http-opts"]["headers"]["Host"][1].as_str(),
        Some("cdn-2.example")
    );
}

#[test]
fn preserves_vless_certificate_constraints() {
    let url = Url::parse(concat!(
        "vless://00000000-0000-0000-0000-000000000001@example.com:443?",
        "security=tls&insecure=1&pcs=sha256-fingerprint&vcn=origin.example"
    ))
    .expect("URL");
    let (proxy, _, _, _) = parse_vless(url, None).expect("proxy");
    assert_eq!(proxy["skip-cert-verify"].as_bool(), Some(true));
    assert_eq!(proxy["fingerprint"].as_str(), Some("sha256-fingerprint"));
    assert_eq!(proxy["name-cert-verify"].as_str(), Some("origin.example"));
}

#[test]
fn preserves_vmess_http_transport_options() {
    let payload = serde_json::json!({
        "v": "2",
        "ps": "vmess-http",
        "add": "vmess.example",
        "port": "443",
        "id": "00000000-0000-0000-0000-000000000001",
        "net": "tcp",
        "type": "http",
        "path": "/video",
        "host": "cdn.example, cdn-2.example",
        "insecure": "1",
        "alpn": "h2,http/1.1",
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
    let url = Url::parse(&format!("vmess://{encoded}")).expect("URL");
    let (proxy, _, _, _) = parse_vmess(url, None, &encoded).expect("proxy");
    assert_eq!(proxy["network"].as_str(), Some("http"));
    assert_eq!(proxy["http-opts"]["path"][0].as_str(), Some("/video"));
    assert_eq!(
        proxy["http-opts"]["headers"]["Host"][0].as_str(),
        Some("cdn.example")
    );
    assert_eq!(
        proxy["http-opts"]["headers"]["Host"][1].as_str(),
        Some("cdn-2.example")
    );
    assert_eq!(proxy["skip-cert-verify"].as_bool(), Some(true));
    assert_eq!(proxy["alpn"][0].as_str(), Some("h2"));
    assert_eq!(proxy["alpn"][1].as_str(), Some("http/1.1"));

    let multiple = serde_json::json!({
        "add": "vmess.example",
        "port": "443",
        "id": "00000000-0000-0000-0000-000000000001",
        "pcs": "first,second",
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(multiple.to_string());
    let url = Url::parse(&format!("vmess://{encoded}")).expect("URL");
    let error = parse_vmess(url, None, &encoded).expect_err("multiple values must fail");
    assert!(error.contains("pcs contains multiple values"));
}

#[test]
fn preserves_vmess_certificate_constraints_without_multi_host_sni() {
    let payload = serde_json::json!({
        "add": "vmess.example",
        "port": "443",
        "id": "00000000-0000-0000-0000-000000000001",
        "net": "tcp",
        "tls": "tls",
        "host": "cdn.example,cdn-2.example",
        "fp": "chrome",
        "pcs": "sha256-fingerprint",
        "vcn": "origin.example",
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
    let url = Url::parse(&format!("vmess://{encoded}")).expect("URL");
    let (proxy, _, _, _) = parse_vmess(url, None, &encoded).expect("proxy");
    assert!(proxy.get("servername").is_none());
    assert_eq!(proxy["client-fingerprint"].as_str(), Some("chrome"));
    assert_eq!(proxy["fingerprint"].as_str(), Some("sha256-fingerprint"));
    assert_eq!(proxy["name-cert-verify"].as_str(), Some("origin.example"));
}

#[test]
fn rejects_vmess_unsupported_tls_mode() {
    let payload = serde_json::json!({
        "add": "vmess.example",
        "port": "443",
        "id": "00000000-0000-0000-0000-000000000001",
        "net": "tcp",
        "tls": "reality",
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
    let url = Url::parse(&format!("vmess://{encoded}")).expect("URL");
    let error = parse_vmess(url, None, &encoded).expect_err("unsupported TLS must fail");
    assert_eq!(error, "vmess tls mode is unsupported");
}

#[test]
fn preserves_trojan_alpn_and_insecure_alias() {
    let url = Url::parse(concat!(
        "trojan://secret@example.com:443?alpn=h2%2Chttp%2F1.1&",
        "insecure=1&pcs=sha256-fingerprint&vcn=origin.example"
    ))
    .expect("URL");
    let (proxy, _, _, _) = parse_trojan(url, None).expect("proxy");
    assert_eq!(proxy["skip-cert-verify"].as_bool(), Some(true));
    assert_eq!(proxy["alpn"][0].as_str(), Some("h2"));
    assert_eq!(proxy["alpn"][1].as_str(), Some("http/1.1"));
    assert_eq!(proxy["fingerprint"].as_str(), Some("sha256-fingerprint"));
    assert_eq!(proxy["name-cert-verify"].as_str(), Some("origin.example"));
}

#[test]
fn preserves_vmess_grpc_mode() {
    let payload = serde_json::json!({
        "add": "vmess.example",
        "port": "443",
        "id": "00000000-0000-0000-0000-000000000001",
        "net": "grpc",
        "type": "gun",
        "path": "grpc-service",
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
    let url = Url::parse(&format!("vmess://{encoded}")).expect("URL");
    let (proxy, _, _, _) = parse_vmess(url, None, &encoded).expect("proxy");
    assert_eq!(proxy["network"].as_str(), Some("grpc"));
    assert_eq!(
        proxy["grpc-opts"]["grpc-service-name"].as_str(),
        Some("grpc-service")
    );
}

#[test]
fn preserves_tuic_allow_insecure_alias() {
    let url = Url::parse(
        "tuic://00000000-0000-0000-0000-000000000001:secret@tuic.example:443?allow_insecure=1",
    )
    .expect("URL");
    let (proxy, _, _, _) = parse_tuic(url, None).expect("proxy");
    assert_eq!(proxy["skip-cert-verify"].as_bool(), Some(true));
}

#[test]
fn preserves_tuic_v4_token() {
    let url = Url::parse("tuic://example-token@tuic.example:443#v4").expect("URL");
    let (proxy, _, _, _) = parse_tuic(url, None).expect("proxy");
    assert_eq!(proxy["token"].as_str(), Some("example-token"));
    assert!(proxy.get("uuid").is_none());
    assert!(proxy.get("password").is_none());
}
