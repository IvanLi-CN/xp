use super::*;
use crate::cluster_identity::generate_cluster_ca;
use crate::config::{Config, DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE, XrayRestartMode};
use axum::routing::get;
use http_body_util::BodyExt;
use rcgen::{
    CertificateParams, DistinguishedName, DnType, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256,
};
use rustls::crypto::aws_lc_rs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{net::SocketAddr, sync::Once};
use tempfile::tempdir;
use time::OffsetDateTime;

static RUSTLS_PROVIDER: Once = Once::new();

fn install_test_crypto_provider() {
    RUSTLS_PROVIDER.call_once(|| {
        let _ = aws_lc_rs::default_provider().install_default();
    });
}

fn test_config(data_dir: PathBuf) -> Config {
    Config {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        xray_api_addr: SocketAddr::from(([127, 0, 0, 1], 10085)),
        xray_health_interval_secs: 5,
        xray_health_fails_before_down: 4,
        xray_restart_mode: XrayRestartMode::None,
        xray_restart_cooldown_secs: 30,
        xray_restart_timeout_secs: 20,
        xray_systemd_unit: "xray.service".to_string(),
        xray_openrc_service: "xray".to_string(),
        cloudflared_health_interval_secs: 5,
        cloudflared_health_fails_before_down: 3,
        cloudflared_monitor_mode: Some(XrayRestartMode::None),
        cloudflared_restart_mode: XrayRestartMode::None,
        cloudflared_restart_cooldown_secs: 30,
        cloudflared_restart_timeout_secs: 20,
        cloudflared_systemd_unit: "cloudflared.service".to_string(),
        cloudflared_openrc_service: "cloudflared".to_string(),
        data_dir,
        admin_token_hash: "hash".to_string(),
        node_name: "node-1".to_string(),
        access_host: "example.com".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        vless_canary_bind: SocketAddr::from(([127, 0, 0, 1], 39043)),
        vless_canary_acme_directory_url: LETS_ENCRYPT_PRODUCTION_URL.to_string(),
        vless_canary_acme_contact_email: String::new(),
        vless_canary_cloudflare_token_file: DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE.to_string(),
        vless_canary_cloudflare_zone_id: String::new(),
        vless_canary_dns_propagation_timeout_secs: 180,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        mesh_proxy_url: None,
        cloudflare_ddns_enabled: false,
        cloudflare_ddns_token_file: DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE.to_string(),
        cloudflare_ddns_zone_id: String::new(),
        cloudflare_ddns_ipv4_url: crate::public_ip_probe::DEFAULT_TRACE_URL.to_string(),
        cloudflare_ddns_ipv6_url: crate::public_ip_probe::DEFAULT_TRACE_URL.to_string(),
        cloudflare_ddns_interval_secs_with_monitor: 300,
        cloudflare_ddns_interval_secs_no_monitor: 60,
        cloudflare_ddns_fast_interval_secs: 30,
        cloudflare_ddns_fast_window_secs: 300,
        cloudflare_ddns_family_missing_grace: 3,
        endpoint_probe_skip_self_test: false,
        quota_poll_interval_secs: 10,
        quota_auto_unban: true,
        ip_geo_enabled: false,
        ip_geo_origin: "https://api.country.is".to_string(),
    }
}

#[test]
fn persist_disabled_status_with_error_records_error() {
    let tmp = tempdir().unwrap();
    let bind: std::net::SocketAddr = "127.0.0.1:39043".parse().unwrap();

    persist_disabled_status_with_error(tmp.path(), bind, "dns setup failed").unwrap();

    let status = load_status(tmp.path(), bind);
    assert!(!status.enabled);
    assert_eq!(status.bind.as_deref(), Some("127.0.0.1:39043"));
    assert_eq!(status.last_error.as_deref(), Some("dns setup failed"));
}

#[test]
fn ready_for_managed_vless_rejects_status_for_different_bind() {
    let tmp = tempdir().unwrap();
    let expected_bind: std::net::SocketAddr = "127.0.0.1:39043".parse().unwrap();
    let stale_bind: std::net::SocketAddr = "127.0.0.1:49043".parse().unwrap();

    persist_status(
        tmp.path(),
        &VlessHttpsCanaryStatus {
            enabled: true,
            bind: Some(stale_bind.to_string()),
            acme_directory_url: Some(LETS_ENCRYPT_PRODUCTION_URL.to_string()),
            cert_not_after: Some("2030-01-01T00:00:00Z".to_string()),
            last_renewed_at: None,
            last_error: None,
        },
    )
    .unwrap();

    assert!(!ready_for_managed_vless(tmp.path(), expected_bind));
}

#[test]
fn effective_zone_id_prefers_explicit_canary_zone() {
    let mut config = test_config(tempdir().unwrap().path().to_path_buf());
    config.cloudflare_ddns_zone_id = "ddns-zone".to_string();
    config.vless_canary_cloudflare_zone_id = "canary-zone".to_string();

    assert_eq!(effective_vless_canary_zone_id(&config), "canary-zone");
}

#[test]
fn effective_zone_id_falls_back_to_ddns_zone() {
    let mut config = test_config(tempdir().unwrap().path().to_path_buf());
    config.cloudflare_ddns_zone_id = "ddns-zone".to_string();
    config.vless_canary_cloudflare_zone_id = String::new();

    assert_eq!(effective_vless_canary_zone_id(&config), "ddns-zone");
}

#[test]
fn ensure_fqdn_appends_trailing_dot_once() {
    assert_eq!(ensure_fqdn("example.com"), "example.com.");
    assert_eq!(ensure_fqdn("example.com."), "example.com.");
}

#[test]
fn zone_name_candidates_walks_toward_zone_apex() {
    assert_eq!(
        zone_name_candidates("_acme-challenge.foo.example.com."),
        vec![
            "_acme-challenge.foo.example.com".to_string(),
            "foo.example.com".to_string(),
            "example.com".to_string(),
            "com".to_string(),
        ]
    );
}

#[test]
fn normalize_authority_defaults_tls_port_and_lowercases_host() {
    assert_eq!(
        normalize_authority("Tokyo.EXAMPLE.com").unwrap(),
        NormalizedAuthority {
            host: "tokyo.example.com".to_string(),
            port: 443,
        }
    );
    assert_eq!(
        normalize_authority("Tokyo.EXAMPLE.com:53844").unwrap(),
        NormalizedAuthority {
            host: "tokyo.example.com".to_string(),
            port: 53844,
        }
    );
}

#[test]
fn build_upstream_url_uses_incoming_path_and_query() {
    let incoming: Uri = "/api/items?cursor=abc&limit=20".parse().unwrap();
    let url = build_upstream_url("http://127.0.0.1:8080", &incoming).unwrap();
    assert_eq!(
        url.as_str(),
        "http://127.0.0.1:8080/api/items?cursor=abc&limit=20"
    );
}

#[test]
fn response_header_filter_preserves_websocket_handshake_headers_only_for_upgrade() {
    assert!(!response_header_allowed("connection", false));
    assert!(!response_header_allowed("upgrade", false));
    assert!(response_header_allowed("connection", true));
    assert!(response_header_allowed("upgrade", true));
}

#[test]
fn websocket_proxy_rejects_h2c_upstreams() {
    let clients = CanaryProxyClients::new().unwrap();
    let auto_client = clients.for_websocket_mode(CanaryUpstreamMode::Auto);
    let http1_client = clients.for_websocket_mode(CanaryUpstreamMode::Http1);
    let h2c_client = clients.for_websocket_mode(CanaryUpstreamMode::H2c);

    assert!(auto_client.is_some());
    assert!(http1_client.is_some());
    assert!(h2c_client.is_none());
}

#[test]
fn websocket_proxy_forces_auto_mode_to_http1_client() {
    let clients = CanaryProxyClients::new().unwrap();
    let auto_client = clients
        .for_websocket_mode(CanaryUpstreamMode::Auto)
        .unwrap();
    let http1_client = clients.for_mode(CanaryUpstreamMode::Http1);

    assert!(std::ptr::eq(auto_client, http1_client));
}

#[tokio::test]
async fn canary_proxy_client_does_not_follow_redirects() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer).await.unwrap();
        stream
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/private\r\nContent-Length: 0\r\n\r\n",
                )
                .await
                .unwrap();
    });

    let clients = CanaryProxyClients::new().unwrap();
    let url = reqwest::Url::parse(&format!("http://{addr}/redirect")).unwrap();
    let response = send_upstream_request(
        clients.for_mode(CanaryUpstreamMode::Auto),
        Method::GET,
        url,
        &HeaderMap::new(),
        Body::empty(),
        false,
    )
    .await
    .unwrap();

    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response.headers().get("location").unwrap(),
        "http://127.0.0.1:9/private"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn canary_proxy_client_uses_upstream_origin_host_header() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buffer = [0_u8; 2048];
        let n = stream.read(&mut buffer).await.unwrap();
        let request = String::from_utf8_lossy(&buffer[..n]);
        assert!(request.contains(&format!("\r\nhost: {addr}\r\n")));
        assert!(!request.contains("\r\nhost: public.example.com\r\n"));
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
    });

    let mut headers = HeaderMap::new();
    headers.insert(HOST, HeaderValue::from_static("public.example.com"));
    let clients = CanaryProxyClients::new().unwrap();
    let url = reqwest::Url::parse(&format!("http://{addr}/")).unwrap();
    let response = send_upstream_request(
        clients.for_mode(CanaryUpstreamMode::Auto),
        Method::GET,
        url,
        &headers,
        Body::empty(),
        false,
    )
    .await
    .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    server.await.unwrap();
}

#[tokio::test]
async fn canary_proxy_client_allows_slow_streaming_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer).await.unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 16\r\n\r\n",
            )
            .await
            .unwrap();
        stream.write_all(b"data: 1\n\n").await.unwrap();
        tokio::time::sleep(Duration::from_millis(750)).await;
        stream.write_all(b"data: 2\n\n").await.unwrap();
    });

    let clients = CanaryProxyClients::new().unwrap();
    let url = reqwest::Url::parse(&format!("http://{addr}/events")).unwrap();
    let response = send_upstream_request(
        clients.for_mode(CanaryUpstreamMode::Auto),
        Method::GET,
        url,
        &HeaderMap::new(),
        Body::empty(),
        false,
    )
    .await
    .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.contains("data: 1"));
    assert!(body.contains("data: 2"));
    server.await.unwrap();
}

#[test]
fn managed_vless_matching_keeps_unconfigured_upstream_diagnostic() {
    let endpoint = Endpoint {
        endpoint_id: "ep1".to_string(),
        node_id: "n1".to_string(),
        tag: "vless-ep1".to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 53844,
        meta: serde_json::json!({
            "reality": {
                "dest": "127.0.0.1:39043",
                "server_names": ["node.example.com"],
                "server_names_source": "manual",
                "fingerprint": "chrome"
            },
            "reality_keys": {
                "private_key": "private",
                "public_key": "public"
            },
            "short_ids": ["aaaaaaaaaaaaaaaa"],
            "active_short_id": "aaaaaaaaaaaaaaaa",
            "managed_default": true
        }),
    };

    let routed = matching_managed_vless_endpoint(
        endpoint,
        "node.example.com",
        &NormalizedAuthority {
            host: "node.example.com".to_string(),
            port: 53844,
        },
    )
    .unwrap();
    assert_eq!(routed.endpoint_id, "ep1");
    assert!(routed.upstream.url.is_empty());
    assert_eq!(routed.upstream.mode, CanaryUpstreamMode::Auto);
}

#[test]
fn managed_vless_matching_requires_managed_default_flag_and_port() {
    let mut endpoint = Endpoint {
        endpoint_id: "ep1".to_string(),
        node_id: "n1".to_string(),
        tag: "vless-ep1".to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 53844,
        meta: serde_json::json!({
            "reality": {
                "dest": "127.0.0.1:39043",
                "server_names": ["node.example.com"],
                "server_names_source": "manual",
                "fingerprint": "chrome"
            },
            "reality_keys": {
                "private_key": "private",
                "public_key": "public"
            },
            "short_ids": ["aaaaaaaaaaaaaaaa"],
            "active_short_id": "aaaaaaaaaaaaaaaa",
            "accepted_authorities": ["edge.example.com:53844"],
            "canary_upstream": {
                "url": "http://127.0.0.1:8080",
                "mode": "h2c"
            },
            "managed_default": false
        }),
    };

    let requested = NormalizedAuthority {
        host: "edge.example.com".to_string(),
        port: 53844,
    };
    assert!(
        matching_managed_vless_endpoint(endpoint.clone(), "node.example.com", &requested).is_none()
    );
    endpoint.meta["managed_default"] = serde_json::Value::Bool(true);
    assert!(
        matching_managed_vless_endpoint(
            endpoint.clone(),
            "node.example.com",
            &NormalizedAuthority {
                host: "node.example.com".to_string(),
                port: 443,
            },
        )
        .is_none()
    );
    let routed = matching_managed_vless_endpoint(endpoint, "node.example.com", &requested).unwrap();
    assert_eq!(routed.upstream.url, "http://127.0.0.1:8080");
    assert_eq!(routed.upstream.mode, CanaryUpstreamMode::H2c);
}

#[test]
fn managed_vless_matching_accepts_alias_without_explicit_port_as_https_443() {
    let endpoint = Endpoint {
        endpoint_id: "ep1".to_string(),
        node_id: "n1".to_string(),
        tag: "vless-ep1".to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::json!({
            "reality": {
                "dest": "127.0.0.1:39043",
                "server_names": ["node.example.com"],
                "server_names_source": "manual",
                "fingerprint": "chrome"
            },
            "reality_keys": {
                "private_key": "private",
                "public_key": "public"
            },
            "short_ids": ["aaaaaaaaaaaaaaaa"],
            "active_short_id": "aaaaaaaaaaaaaaaa",
            "accepted_authorities": ["Edge.Example.com."],
            "canary_upstream": {
                "url": "http://127.0.0.1:8080",
                "mode": "auto"
            },
            "managed_default": true
        }),
    };

    let routed = matching_managed_vless_endpoint(
        endpoint,
        "node.example.com",
        &NormalizedAuthority {
            host: "edge.example.com".to_string(),
            port: 443,
        },
    )
    .unwrap();
    assert_eq!(routed.endpoint_id, "ep1");
}

#[test]
fn managed_vless_matching_rejects_non_canonical_non_alias_authority() {
    let endpoint = Endpoint {
        endpoint_id: "ep1".to_string(),
        node_id: "n1".to_string(),
        tag: "vless-ep1".to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 53844,
        meta: serde_json::json!({
            "reality": {
                "dest": "127.0.0.1:39043",
                "server_names": ["node.example.com"],
                "server_names_source": "manual",
                "fingerprint": "chrome"
            },
            "reality_keys": {
                "private_key": "private",
                "public_key": "public"
            },
            "short_ids": ["aaaaaaaaaaaaaaaa"],
            "active_short_id": "aaaaaaaaaaaaaaaa",
            "accepted_authorities": ["edge.example.com:53844"],
            "managed_default": true
        }),
    };

    assert!(
        matching_managed_vless_endpoint(
            endpoint,
            "node.example.com",
            &NormalizedAuthority {
                host: "other.example.com".to_string(),
                port: 53844,
            },
        )
        .is_none()
    );
}

#[test]
fn managed_vless_matching_accepts_canonical_authority() {
    let endpoint = Endpoint {
        endpoint_id: "ep1".to_string(),
        node_id: "n1".to_string(),
        tag: "vless-ep1".to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::json!({
            "reality": {
                "dest": "127.0.0.1:39043",
                "server_names": ["node.example.com"],
                "server_names_source": "manual",
                "fingerprint": "chrome"
            },
            "reality_keys": {
                "private_key": "private",
                "public_key": "public"
            },
            "short_ids": ["aaaaaaaaaaaaaaaa"],
            "active_short_id": "aaaaaaaaaaaaaaaa",
            "managed_default": true
        }),
    };

    let routed = matching_managed_vless_endpoint(
        endpoint,
        "Node.Example.com.",
        &NormalizedAuthority {
            host: "node.example.com".to_string(),
            port: 443,
        },
    )
    .unwrap();
    assert_eq!(routed.endpoint_id, "ep1");
}

#[tokio::test]
async fn not_found_response_is_plain_text_404() {
    let response = not_found_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), b"404 Not Found");
}

#[cfg(unix)]
#[test]
fn write_atomic_key_material_is_chmodded_0600() {
    let tmp = tempdir().unwrap();
    let paths = VlessHttpsCanaryPaths::new(tmp.path());
    fs::create_dir_all(&paths.dir).unwrap();

    write_atomic(&paths.account_key_pem, b"account-key").unwrap();
    best_effort_chmod_0600(&paths.account_key_pem);
    write_atomic(&paths.key_pem, b"tls-key").unwrap();
    best_effort_chmod_0600(&paths.key_pem);

    let account_mode = fs::metadata(&paths.account_key_pem)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let key_mode = fs::metadata(&paths.key_pem).unwrap().permissions().mode() & 0o777;

    assert_eq!(account_mode, 0o600);
    assert_eq!(key_mode, 0o600);
}

#[tokio::test]
async fn wait_until_ready_accepts_self_signed_canary_cert() {
    install_test_crypto_provider();

    let ca = generate_cluster_ca("cluster-1").unwrap();
    let ca_key = KeyPair::from_pem(&ca.key_pem).unwrap();
    let ca_cert = Issuer::from_ca_cert_pem(&ca.cert_pem, ca_key).unwrap();

    let mut params = CertificateParams::new(vec!["canary.example.com".to_string()]).unwrap();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "canary.example.com");
    params.distinguished_name = dn;
    let now = OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(30);

    let cert_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = params.signed_by(&cert_key, &ca_cert).unwrap();
    let cert_pem = cert.pem();
    let key_pem = cert_key.serialize_pem();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let bind = listener.local_addr().unwrap();
    let rustls = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert_pem.into_bytes(),
        key_pem.into_bytes(),
    )
    .await
    .unwrap();

    let app = Router::new().route(
        GENERATE_204_PATH,
        get(|| async { StatusCode::NO_CONTENT.into_response() }),
    );
    let server = axum_server::from_tcp_rustls(listener, rustls)
        .unwrap()
        .serve(app.into_make_service());
    let handle = tokio::spawn(server.into_future());

    let result = wait_until_ready("canary.example.com", bind, 5, Duration::from_millis(100)).await;

    handle.abort();

    assert!(result.is_ok(), "unexpected readiness error: {result:?}");
}

#[test]
fn authoritative_txt_policy_requires_all_reachable_ips_to_match() {
    fn reduce(results: &[Result<bool, ()>]) -> bool {
        let mut saw_reachable = false;
        for result in results {
            match result {
                Ok(true) => {
                    saw_reachable = true;
                }
                Ok(false) => {
                    return false;
                }
                Err(()) => continue,
            }
        }
        saw_reachable
    }

    assert!(!reduce(&[Ok(true), Ok(false)]));
    assert!(reduce(&[Ok(true), Err(())]));
    assert!(!reduce(&[Err(()), Err(())]));
}

#[test]
fn parse_openssl_not_after_accepts_double_digit_day() {
    let parsed =
        parse_openssl_not_after("Sep 16 09:13:04 2026 GMT").expect("double-digit day should parse");
    assert_eq!(parsed.to_rfc3339(), "2026-09-16T09:13:04+00:00");
}
