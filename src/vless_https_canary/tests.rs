use super::*;
use crate::cluster_identity::generate_cluster_ca;
use crate::config::{Config, DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE, XrayRestartMode};
use crate::domain::{Node, NodeQuotaReset};
use crate::internal_auth::{InternalRoute, RequestContext};
use crate::state::StoreInit;
use axum::routing::get;
use http_body_util::BodyExt;
use rcgen::{
    CertificateParams, DistinguishedName, DnType, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256,
};
use rustls::crypto::ring;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{net::SocketAddr, sync::Once};
use tempfile::tempdir;
use time::OffsetDateTime;
use trust_dns_resolver::config::Protocol;

static RUSTLS_PROVIDER: Once = Once::new();

fn install_test_crypto_provider() {
    RUSTLS_PROVIDER.call_once(|| {
        let _ = ring::default_provider().install_default();
    });
}

fn test_config(data_dir: PathBuf) -> Config {
    Config {
        bind: xp_test_fixtures::address_loopback_port0().parse().unwrap(),
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
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::host_fixture465().to_owned(),
        api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
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
    let _stale_bind: std::net::SocketAddr = "127.0.0.1:49043".parse().unwrap();

    persist_status(
        tmp.path(),
        &VlessHttpsCanaryStatus {
            enabled: true,
            bind: Some(xp_test_fixtures::address_loopback_port39466().to_owned()),
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
fn build_upstream_url_ignores_http2_absolute_uri_origin() {
    let incoming: Uri = "https://mesh.example.com/api/items/a%2Fb?cursor=a%2Fb&limit=20"
        .parse()
        .unwrap();
    let url = build_upstream_url("http://127.0.0.1:8080", &incoming).unwrap();

    assert_eq!(
        url.as_str(),
        "http://127.0.0.1:8080/api/items/a%2Fb?cursor=a%2Fb&limit=20"
    );
}

#[test]
fn mesh_loopback_url_rejects_authenticated_uri_normalization() {
    for incoming in [
        "/api/admin/_internal/mesh/../health",
        "/api/admin/_internal/mesh/%2e%2e/health",
        "/api/admin/_internal/mesh\\health",
    ] {
        let incoming: Uri = incoming.parse().unwrap();
        assert!(mesh::build_mesh_loopback_url("http://127.0.0.1:8080", &incoming).is_err());
    }
}

#[test]
fn response_header_filter_preserves_websocket_handshake_headers_only_for_upgrade() {
    assert!(!response_header_allowed("connection", false));
    assert!(!response_header_allowed("upgrade", false));
    assert!(response_header_allowed("connection", true));
    assert!(response_header_allowed("upgrade", true));
}

#[test]
fn reserved_mesh_headers_never_enter_camouflage_forwarding() {
    let mut headers = HeaderMap::new();
    headers.insert(
        internal_auth::INTERNAL_ROUTE_HEADER,
        HeaderValue::from_static("mesh-v2"),
    );
    assert!(is_reserved_mesh_request(&headers));
    assert!(!request_header_allowed(
        internal_auth::INTERNAL_ROUTE_HEADER,
        false
    ));
    assert!(!request_header_allowed(
        internal_auth::INTERNAL_SIGNATURE_HEADER,
        false
    ));
}

#[test]
fn health_v2_mesh_ingress_requires_an_empty_body() {
    assert!(mesh::permitted_mesh_ingress(
        Some("health-v2"),
        &Method::GET,
        "/api/admin/_internal/mesh/health",
        0,
    ));
    assert!(!mesh::permitted_mesh_ingress(
        Some("health-v2"),
        &Method::GET,
        "/api/admin/_internal/mesh/health",
        1,
    ));
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
        endpoint_id: xp_test_fixtures::label_vless1().to_owned(),
        node_id: xp_test_fixtures::label_n1().to_owned(),
        tag: xp_test_fixtures::label_vless1().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 53844,
        meta: serde_json::json!({
            "reality": xp_test_fixtures::endpoint_reality(),
            "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
            "short_ids": xp_test_fixtures::endpoint_short_ids(),
            "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
            "managed_default": true
        }),
    };

    let routed = matching_managed_vless_endpoint(
        endpoint,
        xp_test_fixtures::primary_host(),
        &NormalizedAuthority {
            host: xp_test_fixtures::label_node_afixture_test().to_owned(),
            port: 53844,
        },
    )
    .unwrap();
    assert_eq!(routed.endpoint_id, xp_test_fixtures::label_vless1());
    assert!(routed.upstream.url.is_empty());
    assert_eq!(routed.upstream.mode, CanaryUpstreamMode::Auto);
}

#[test]
fn managed_vless_matching_requires_managed_default_flag_and_port() {
    let mut endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::label_vless1().to_owned(),
        node_id: xp_test_fixtures::label_n1().to_owned(),
        tag: xp_test_fixtures::label_vless1().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 53844,
        meta: serde_json::json!({
            "reality": xp_test_fixtures::endpoint_reality(),
            "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
            "short_ids": xp_test_fixtures::endpoint_short_ids(),
            "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
            "accepted_authorities": [xp_test_fixtures::endpoint_authority_53844()],
            "canary_upstream": {
                "url": xp_test_fixtures::canary_http_loopback_url(),
                "mode": xp_test_fixtures::endpoint_canary_h2c()
            },
            "managed_default": false
        }),
    };

    let requested = NormalizedAuthority {
        host: xp_test_fixtures::label_edge_afixture_test().to_owned(),
        port: 53844,
    };
    assert!(
        matching_managed_vless_endpoint(
            endpoint.clone(),
            xp_test_fixtures::primary_host(),
            &requested
        )
        .is_none()
    );
    endpoint.meta["managed_default"] = serde_json::Value::Bool(true);
    assert!(
        matching_managed_vless_endpoint(
            endpoint.clone(),
            xp_test_fixtures::primary_host(),
            &NormalizedAuthority {
                host: "node.example.com".to_string(),
                port: 443,
            },
        )
        .is_none()
    );
    let routed =
        matching_managed_vless_endpoint(endpoint, xp_test_fixtures::primary_host(), &requested)
            .unwrap();
    assert_eq!(routed.upstream.url, "http://127.0.0.1:8080");
    assert_eq!(routed.upstream.mode, CanaryUpstreamMode::H2c);
}

#[test]
fn managed_vless_matching_accepts_alias_without_explicit_port_as_https_443() {
    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::label_vless1().to_owned(),
        node_id: xp_test_fixtures::label_n1().to_owned(),
        tag: xp_test_fixtures::label_vless1().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::json!({
            "reality": xp_test_fixtures::endpoint_reality(),
            "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
            "short_ids": xp_test_fixtures::endpoint_short_ids(),
            "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
            "accepted_authorities": [xp_test_fixtures::endpoint_authority_alias()],
            "canary_upstream": {
                "url": xp_test_fixtures::canary_http_loopback_url(),
                "mode": "auto"
            },
            "managed_default": true
        }),
    };

    let routed = matching_managed_vless_endpoint(
        endpoint,
        xp_test_fixtures::primary_host(),
        &NormalizedAuthority {
            host: xp_test_fixtures::label_edge_afixture_test().to_owned(),
            port: 443,
        },
    )
    .unwrap();
    assert_eq!(routed.endpoint_id, xp_test_fixtures::label_vless1());
}

#[test]
fn managed_vless_matching_rejects_non_canonical_non_alias_authority() {
    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::label_vless1().to_owned(),
        node_id: xp_test_fixtures::label_n1().to_owned(),
        tag: xp_test_fixtures::label_vless1().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 53844,
        meta: serde_json::json!({
            "reality": xp_test_fixtures::endpoint_reality(),
            "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
            "short_ids": xp_test_fixtures::endpoint_short_ids(),
            "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
            "accepted_authorities": [xp_test_fixtures::endpoint_authority_53844()],
            "managed_default": true
        }),
    };

    assert!(
        matching_managed_vless_endpoint(
            endpoint,
            xp_test_fixtures::primary_host(),
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
        endpoint_id: xp_test_fixtures::label_vless1().to_owned(),
        node_id: xp_test_fixtures::label_n1().to_owned(),
        tag: xp_test_fixtures::label_vless1().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::json!({
            "reality": xp_test_fixtures::endpoint_reality(),
            "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
            "short_ids": xp_test_fixtures::endpoint_short_ids(),
            "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
            "managed_default": true
        }),
    };

    let routed = matching_managed_vless_endpoint(
        endpoint,
        "Node-A.Fixture.Test.",
        &NormalizedAuthority {
            host: xp_test_fixtures::label_node_afixture_test().to_owned(),
            port: 443,
        },
    )
    .unwrap();
    assert_eq!(routed.endpoint_id, xp_test_fixtures::label_vless1());
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

#[tokio::test]
async fn signed_mesh_health_reaches_loopback_over_http1_and_http2() {
    install_test_crypto_provider();

    const HEALTH_PATH: &str = "/api/admin/_internal/mesh/health?probe=a%2Fb";

    let cluster_id = xp_test_fixtures::cluster_fixture476();
    let sender_id = xp_test_fixtures::node_id_fixture472();
    let target_id = xp_test_fixtures::node_id_fixture475();
    let ca = generate_cluster_ca(cluster_id).unwrap();
    let request_uri: Uri = HEALTH_PATH.parse().unwrap();
    let context = RequestContext::now(
        InternalRoute::HealthV2,
        cluster_id,
        sender_id,
        target_id,
        "01JTESTREQUEST000000000000000",
    );
    let mut signed_headers = HeaderMap::new();
    internal_auth::sign_request_v2(
        &ca.key_pem,
        &ca.cert_pem,
        &Method::GET,
        &request_uri,
        None,
        &[],
        &context,
        &mut signed_headers,
    )
    .unwrap();
    let verified = internal_auth::verify_request_v2(
        &ca.key_pem,
        &ca.cert_pem,
        &Method::GET,
        &request_uri,
        &signed_headers,
        &[],
        cluster_id,
        target_id,
    )
    .unwrap();
    let ack = internal_auth::sign_ack_v2(
        &ca.key_pem,
        &ca.cert_pem,
        &verified,
        target_id,
        StatusCode::OK.as_u16(),
    )
    .unwrap();

    let loopback_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let loopback_addr = loopback_listener.local_addr().unwrap();
    let expected_uri = request_uri.clone();
    let loopback_app = Router::new().fallback(move |uri: Uri| {
        let ack = ack.clone();
        let expected_uri = expected_uri.clone();
        async move {
            assert_eq!(uri.path_and_query(), expected_uri.path_and_query());
            Response::builder()
                .status(StatusCode::OK)
                .header(internal_auth::INTERNAL_ACK_HEADER, ack)
                .body(Body::empty())
                .unwrap()
        }
    });
    let loopback_server = tokio::spawn(async move {
        axum::serve(loopback_listener, loopback_app).await.unwrap();
    });

    let tmp = tempdir().unwrap();
    let mut store = JsonSnapshotStore::load_or_init(StoreInit {
        data_dir: tmp.path().join("store"),
        bootstrap_node_id: Some(xp_test_fixtures::node_id_fixture475().to_owned()),
        bootstrap_node_name: xp_test_fixtures::label_target().to_owned(),
        bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
        bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
    })
    .unwrap();
    store
        .upsert_node(Node {
            node_id: xp_test_fixtures::node_id_fixture472().to_owned(),
            node_name: xp_test_fixtures::label_sender().to_owned(),
            access_host: xp_test_fixtures::host_fixture473().to_owned(),
            api_base_url: xp_test_fixtures::service_fixture474().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();

    let ca_key = KeyPair::from_pem(&ca.key_pem).unwrap();
    let ca_cert = Issuer::from_ca_cert_pem(&ca.cert_pem, ca_key).unwrap();
    let cert_params = CertificateParams::new(vec!["canary.example.com".to_string()]).unwrap();
    let cert_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = cert_params.signed_by(&cert_key, &ca_cert).unwrap();
    let rustls = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert.pem().into_bytes(),
        cert_key.serialize_pem().into_bytes(),
    )
    .await
    .unwrap();
    let canary_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    canary_listener.set_nonblocking(true).unwrap();
    let canary_addr = canary_listener.local_addr().unwrap();
    let state = CanaryProxyState {
        store: Arc::new(tokio::sync::Mutex::new(store)),
        node_id: xp_test_fixtures::node_id_fixture475().to_owned(),
        clients: Arc::new(CanaryProxyClients::new().unwrap()),
        mesh_auth: Some(CanaryMeshAuth {
            cluster_id: xp_test_fixtures::cluster_fixture476().to_owned(),
            cluster_ca_key_pem: ca.key_pem.clone(),
            cluster_ca_cert_pem: ca.cert_pem.clone(),
            loopback_base_url: format!("http://{loopback_addr}"),
        }),
    };
    let canary_app = Router::new().fallback(canary_proxy).with_state(state);
    let canary_server = axum_server::from_tcp_rustls(canary_listener, rustls)
        .unwrap()
        .serve(canary_app.into_make_service());
    let canary_handle = tokio::spawn(canary_server.into_future());

    for http2 in [false, true] {
        let mut client = reqwest::Client::builder()
            .resolve("canary.example.com", canary_addr)
            .danger_accept_invalid_certs(true);
        client = if http2 {
            client.http2_prior_knowledge()
        } else {
            client.http1_only()
        };
        let response = client
            .build()
            .unwrap()
            .get(format!("https://canary.example.com{HEALTH_PATH}"))
            .headers(signed_headers.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "http2={http2}");
        assert_eq!(
            response.version(),
            if http2 {
                axum::http::Version::HTTP_2
            } else {
                axum::http::Version::HTTP_11
            }
        );
        let response_ack = response
            .headers()
            .get(internal_auth::INTERNAL_ACK_HEADER)
            .unwrap()
            .to_str()
            .unwrap();
        internal_auth::verify_ack_v2(
            &ca.key_pem,
            &ca.cert_pem,
            &verified,
            target_id,
            response.status().as_u16(),
            response_ack,
        )
        .unwrap();
    }

    canary_handle.abort();
    loopback_server.abort();
}

#[test]
fn dns_propagation_uses_only_dns_over_https_resolvers() {
    let resolvers = dns_over_https_propagation_resolvers();

    assert_eq!(
        resolvers.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        ["cloudflare", "google"]
    );
    for (_, config) in resolvers {
        assert!(!config.name_servers().is_empty());
        assert!(config.name_servers().iter().all(|server| {
            server.protocol == Protocol::Https && !server.trust_negative_responses
        }));
    }
}

#[test]
fn parse_openssl_not_after_accepts_double_digit_day() {
    let parsed =
        parse_openssl_not_after("Sep 16 09:13:04 2026 GMT").expect("double-digit day should parse");
    assert_eq!(parsed.to_rfc3339(), "2026-09-16T09:13:04+00:00");
}
