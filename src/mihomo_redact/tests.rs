use super::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn raw_vless_masks_uuid_pbk_sid_but_keeps_host() {
    let input = "vless://12345678-1234-1234-1234-123456789abc@edge.example.com:443?encryption=none&security=reality&type=tcp&sni=example.com&fp=chrome&pbk=public_key_value&sid=0123456789abcdef#node-a\n";
    let out = redact_text(input, RedactionLevel::Credentials);

    assert!(!out.contains("12345678-1234-1234-1234-123456789abc"));
    assert!(!out.contains("public_key_value"));
    assert!(!out.contains("0123456789abcdef"));
    assert!(out.contains("edge.example.com:443"));
}

#[test]
fn raw_ss_masks_password_but_keeps_method() {
    let input = "ss://2022-blake3-aes-128-gcm:AAAAAAAAAAAAAAAAAAAAAA==:BBBBBBBBBBBBBBBBBBBBBB==@edge.example.com:443#node\n";
    let out = redact_text(input, RedactionLevel::Credentials);

    assert!(out.contains("ss://2022-blake3-aes-128-gcm:"));
    assert!(!out.contains("AAAAAAAAAAAAAAAAAAAAAA==:BBBBBBBBBBBBBBBBBBBBBB=="));
    assert!(out.contains("edge.example.com:443"));
}

#[test]
fn url_path_and_query_token_are_redacted() {
    let input = "url: https://example.com/api/sub/sub_token_123456789?token=my_token_value\n";
    let out = redact_text(input, RedactionLevel::Minimal);

    assert!(!out.contains("sub_token_123456789"));
    assert!(!out.contains("my_token_value"));
    assert!(out.contains("https://example.com/api/sub/"));
}

#[test]
fn url_fragment_query_token_is_redacted() {
    let input = "url: https://example.com/api/sub/abcdef#token=mysecret\n";
    let out = redact_text(input, RedactionLevel::Minimal);

    assert!(!out.contains("mysecret"));
    assert!(!out.contains("/api/sub/abcdef"));
}

#[test]
fn url_fragment_path_and_query_token_are_redacted() {
    let input = "url: https://example.com/#/api/sub/abcdef?token=mysecret\n";
    let out = redact_text(input, RedactionLevel::Minimal);

    assert!(!out.contains("/api/sub/abcdef"));
    assert!(!out.contains("mysecret"));
}

#[test]
fn url_fragment_path_with_equals_token_is_redacted() {
    let input = "url: https://example.com/#/api/sub/abc123==\n";
    let out = redact_text(input, RedactionLevel::Minimal);

    assert!(!out.contains("/api/sub/abc123=="));
}

#[test]
fn url_fragment_query_value_with_slash_is_redacted() {
    let input = "url: https://example.com/#token=abc/def\n";
    let out = redact_text(input, RedactionLevel::Minimal);

    assert!(!out.contains("abc/def"));
}

#[test]
fn url_fragment_path_key_value_token_is_redacted() {
    let input = "url: https://example.com/#/token=mysecret\n";
    let out = redact_text(input, RedactionLevel::Minimal);

    assert!(!out.contains("mysecret"));
}

#[test]
fn url_fragment_query_value_embedded_token_is_redacted() {
    let input = "url: https://example.com/#foo=abc/token=mysecret\n";
    let out = redact_text(input, RedactionLevel::Minimal);

    assert!(!out.contains("mysecret"));
}

#[test]
fn yaml_redaction_keeps_comment_indent_and_order() {
    let input = "proxies:\n  - name: edge\n    password: super-secret-value # keep this comment\n";
    let out = redact_text(input, RedactionLevel::Credentials);

    assert!(out.contains("proxies:\n  - name: edge\n    password:"));
    assert!(out.contains("# keep this comment"));
    assert!(!out.contains("super-secret-value"));
}

#[test]
fn yaml_plain_scalar_hash_is_not_treated_as_comment() {
    let input = "password: abc#123\n";
    let out = redact_text(input, RedactionLevel::Credentials);

    assert!(!out.contains("abc#123"));
    assert!(!out.contains("#123"));
}

#[test]
fn yaml_plain_scalar_bracket_hash_is_not_treated_as_comment() {
    let input = "password: abc]#123\n";
    let out = redact_text(input, RedactionLevel::Credentials);

    assert!(!out.contains("abc]#123"));
    assert!(!out.contains("#123"));
}

#[test]
fn yaml_plain_scalar_nested_brackets_hash_is_not_treated_as_comment() {
    let input = "password: a[1]#9\n";
    let out = redact_text(input, RedactionLevel::Credentials);

    assert!(!out.contains("a[1]#9"));
    assert!(!out.contains("#9"));
}

#[test]
fn yaml_flow_sequence_allows_comment_without_space() {
    let input = "password: [abc]#keep\n";
    let out = redact_text(input, RedactionLevel::Credentials);

    assert!(out.contains("#keep"));
    assert!(!out.contains("[abc]"));
}

#[test]
fn yaml_flow_mapping_allows_comment_without_space() {
    let input = "password: {k: v}#keep\n";
    let out = redact_text(input, RedactionLevel::Credentials);

    assert!(out.contains("#keep"));
    assert!(!out.contains("{k: v}"));
}

#[test]
fn yaml_quoted_scalar_allows_comment_without_space() {
    let input = "password: \"abc\"#keep\n";
    let out = redact_text(input, RedactionLevel::Credentials);

    assert!(out.contains("password: \"***\"#keep"));
    assert!(!out.contains("abc"));
}

#[test]
fn credentials_and_address_redacts_server_and_sni() {
    let input = "server: edge.example.com\nservername: reality.example.com\n";
    let out = redact_text(input, RedactionLevel::CredentialsAndAddress);

    assert!(!out.contains("edge.example.com"));
    assert!(!out.contains("reality.example.com"));
}

#[test]
fn auto_detect_base64_subscription_decodes_text() {
    let raw = "vless://12345678-1234-1234-1234-123456789abc@example.com:443?pbk=abc12345&sid=0123456789abcdef#node\n";
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
    let decoded = normalize_input(&encoded, SourceFormat::Auto).unwrap();

    assert_eq!(decoded, raw);
}

#[test]
fn explicit_base64_mode_fails_on_invalid_text() {
    let err = normalize_input("***", SourceFormat::Base64).unwrap_err();
    assert_eq!(err.code, 2);
}

#[test]
fn source_format_raw_keeps_input_unchanged() {
    let input = "not base64 text";
    let out = normalize_input(input, SourceFormat::Raw).unwrap();
    assert_eq!(out, input);
}

#[tokio::test]
async fn permissive_url_loader_fetches_remote_source() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/raw"))
        .respond_with(ResponseTemplate::new(200).set_body_string("vless://demo@example.com:443\n"))
        .mount(&server)
        .await;

    let text = load_text_from_url(&format!("{}/raw", server.uri()), 5, UrlLoadPolicy::AllowAny)
        .await
        .unwrap();

    assert_eq!(text, "vless://demo@example.com:443\n");
}

#[tokio::test]
async fn public_only_url_loader_rejects_loopback_targets() {
    let err = load_text_from_url("http://127.0.0.1:8080/raw", 5, UrlLoadPolicy::PublicOnly)
        .await
        .unwrap_err();

    assert_eq!(err.kind, RedactErrorKind::InvalidInput);
    assert!(err.message.contains("public ip"));
}

#[tokio::test]
async fn public_only_url_loader_rejects_redirects_to_loopback() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/redirect"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "http://127.0.0.1:8080/raw"),
        )
        .mount(&server)
        .await;

    let err = load_text_from_url(
        &format!("{}/redirect", server.uri()),
        5,
        UrlLoadPolicy::PublicOnly,
    )
    .await
    .unwrap_err();

    assert_eq!(err.kind, RedactErrorKind::InvalidInput);
    assert!(err.message.contains("public ip"));
}

#[tokio::test]
async fn url_loader_enforces_total_timeout_across_redirect_chain() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/redirect-1"))
        .respond_with(
            ResponseTemplate::new(302)
                .set_delay(Duration::from_millis(700))
                .insert_header("location", "/redirect-2"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/redirect-2"))
        .respond_with(
            ResponseTemplate::new(302)
                .set_delay(Duration::from_millis(700))
                .insert_header("location", "/raw"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/raw"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok\n"))
        .mount(&server)
        .await;

    let start = std::time::Instant::now();
    let err = load_text_from_url(
        &format!("{}/redirect-1", server.uri()),
        1,
        UrlLoadPolicy::AllowAny,
    )
    .await
    .unwrap_err();

    assert_eq!(err.kind, RedactErrorKind::Network);
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn url_loader_rejects_remote_bodies_that_exceed_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/large"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(vec![b'a'; MAX_REMOTE_SOURCE_BYTES + 1]),
        )
        .mount(&server)
        .await;

    let err = load_text_from_url(
        &format!("{}/large", server.uri()),
        5,
        UrlLoadPolicy::AllowAny,
    )
    .await
    .unwrap_err();

    assert_eq!(err.kind, RedactErrorKind::InvalidInput);
    assert!(err.message.contains("exceeds"));
}

#[tokio::test]
async fn pinned_http_client_uses_validated_socket_addresses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/raw"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok\n"))
        .mount(&server)
        .await;

    let server_url = Url::parse(&server.uri()).unwrap();
    let port = server_url.port().unwrap();
    let addrs = [SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)];
    let client = build_pinned_http_client("example.test", Duration::from_secs(5), &addrs).unwrap();

    let text = client
        .get(format!("http://example.test:{port}/raw"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert_eq!(text, "ok\n");
}

#[test]
fn public_ipv6_filter_accepts_global_unicast_and_rejects_special_ranges() {
    assert!(is_public_ipv6("2606:4700:4700::1111".parse().unwrap()));
    assert!(!is_public_ipv6("fec0::1".parse().unwrap()));
    assert!(!is_public_ipv6("2001:db8::1".parse().unwrap()));
    assert!(!is_public_ipv6("2001::1".parse().unwrap()));
}

#[test]
fn explicit_base64_mode_accepts_missing_padding_and_newlines() {
    let raw = "vless://a@b:1\n";
    let mut encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
    encoded = encoded.trim_end_matches('=').to_string();
    let with_newlines = format!(
        "{}\n{}",
        &encoded[..encoded.len() / 2],
        &encoded[encoded.len() / 2..]
    );

    let decoded = normalize_input(&with_newlines, SourceFormat::Base64).unwrap();
    assert_eq!(decoded, raw);
}

#[test]
fn vmess_opaque_payload_is_redacted() {
    let payload = base64::engine::general_purpose::STANDARD.encode(
            r#"{"v":"2","add":"server.example.com","id":"12345678-90ab-cdef-1234-567890abcdef","ps":"demo"}"#,
        );
    let input = format!("vmess://{payload}");
    let out = redact_text(&input, RedactionLevel::Credentials);
    let encoded = out.strip_prefix("vmess://").unwrap();
    let decoded = decode_base64_bytes_lenient(encoded).unwrap();
    let value = serde_json::from_slice::<serde_json::Value>(&decoded).unwrap();
    let id = value.get("id").and_then(|v| v.as_str()).unwrap();
    let add = value.get("add").and_then(|v| v.as_str()).unwrap();

    assert!(out.starts_with("vmess://"));
    assert_ne!(id, "12345678-90ab-cdef-1234-567890abcdef");
    assert_eq!(add, "server.example.com");
}

#[test]
fn vmess_opaque_payload_redacts_address_at_address_level() {
    let payload = base64::engine::general_purpose::STANDARD.encode(
            r#"{"v":"2","add":"server.example.com","id":"12345678-90ab-cdef-1234-567890abcdef","ps":"demo"}"#,
        );
    let input = format!("vmess://{payload}");
    let out = redact_text(&input, RedactionLevel::CredentialsAndAddress);
    let encoded = out.strip_prefix("vmess://").unwrap();
    let decoded = decode_base64_bytes_lenient(encoded).unwrap();
    let value = serde_json::from_slice::<serde_json::Value>(&decoded).unwrap();
    let add = value.get("add").and_then(|v| v.as_str()).unwrap();

    assert_ne!(add, "server.example.com");
}

#[test]
fn ss_opaque_payload_is_redacted() {
    let payload = base64::engine::general_purpose::STANDARD
        .encode("aes-128-gcm:super-secret@example.com:443");
    let input = format!("ss://{payload}#demo");
    let out = redact_text(&input, RedactionLevel::Credentials);

    assert!(out.starts_with("ss://"));
    assert!(!out.contains("super-secret"));
}
