use std::{ffi::OsString, net::SocketAddr, path::PathBuf, sync::Arc};

use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use base64::Engine as _;
use bytes::Bytes;
use http_body_util::BodyExt;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use serde_yaml::Value as YamlValue;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::sync::{Mutex, mpsc, watch};
use tower::util::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::{
    cloudflared_supervisor::{CloudflaredHealthHandle, CloudflaredStatus},
    cluster_metadata::ClusterMetadata,
    config::Config,
    domain::{EndpointKind, Node, NodeQuotaReset, QuotaResetSource},
    http::build_router,
    id::{is_ulid_string, new_ulid_string},
    protocol::{Ss2022EndpointMeta, ss2022_password},
    raft::{
        app::LocalRaft,
        types::{NodeMeta as RaftNodeMeta, raft_node_id_from_ulid},
    },
    reconcile::{ReconcileHandle, ReconcileRequest},
    state::{JsonSnapshotStore, StoreInit, membership_key},
    xray_supervisor::XrayHealthHandle,
};

fn test_config(data_dir: PathBuf) -> Config {
    let hash = test_admin_token_hash();
    Config {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        xray_api_addr: SocketAddr::from(([127, 0, 0, 1], 10085)),
        xray_health_interval_secs: 2,
        xray_health_fails_before_down: 3,
        xray_restart_mode: crate::config::XrayRestartMode::None,
        xray_restart_cooldown_secs: 30,
        xray_restart_timeout_secs: 5,
        xray_systemd_unit: "xray.service".to_string(),
        xray_openrc_service: "xray".to_string(),
        cloudflared_health_interval_secs: 5,
        cloudflared_health_fails_before_down: 3,
        cloudflared_restart_mode: crate::config::XrayRestartMode::None,
        cloudflared_restart_cooldown_secs: 30,
        cloudflared_restart_timeout_secs: 5,
        cloudflared_systemd_unit: "cloudflared.service".to_string(),
        cloudflared_openrc_service: "cloudflared".to_string(),
        data_dir,
        admin_token_hash: hash,
        node_name: "node-1".to_string(),
        access_host: "".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        endpoint_probe_skip_self_test: false,
        quota_poll_interval_secs: 10,
        quota_auto_unban: true,
        ip_usage_city_db_path: String::new(),
        ip_usage_asn_db_path: String::new(),
    }
}

const TEST_ADMIN_TOKEN: &str = "testtoken";

fn test_admin_token_hash() -> String {
    // Keep tests fast: use a deterministic, low-cost argon2id hash.
    let params = Params::new(32, 1, 1, None).expect("argon2 params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let salt = SaltString::encode_b64(b"xp-test-salt").expect("salt");
    argon2
        .hash_password(TEST_ADMIN_TOKEN.as_bytes(), &salt)
        .expect("hash_password")
        .to_string()
}

fn test_store_init(config: &Config, bootstrap_node_id: Option<String>) -> StoreInit {
    StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id,
        bootstrap_node_name: config.node_name.clone(),
        bootstrap_access_host: config.access_host.clone(),
        bootstrap_api_base_url: config.api_base_url.clone(),
    }
}

fn test_geo_db_update_handle(
    config: &Config,
    store: Arc<Mutex<JsonSnapshotStore>>,
) -> crate::ip_geo_db::GeoDbUpdateHandle {
    let (handle, _task) =
        crate::ip_geo_db::spawn_geo_db_update_worker(Arc::new(config.clone()), store).unwrap();
    handle
}

fn app_with(
    tmp: &TempDir,
    reconcile: ReconcileHandle,
) -> (axum::Router, Arc<Mutex<JsonSnapshotStore>>) {
    let config = test_config(tmp.path().to_path_buf());
    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(tmp.path()).unwrap();

    let store =
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
            .unwrap();
    let store = Arc::new(Mutex::new(store));

    let raft = leader_raft(store.clone(), &cluster);
    let xray_health = XrayHealthHandle::new_unknown();
    let cloudflared_health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
    let (node_runtime, _node_runtime_task) = crate::node_runtime::spawn_node_runtime_monitor(
        Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let geo_db_update = test_geo_db_update_handle(&config, store.clone());
    let router = build_router(
        config,
        store.clone(),
        reconcile,
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
        geo_db_update,
    );
    (router, store)
}

#[tokio::test]
async fn health_is_200_and_includes_xray_fields() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req("GET", "/api/health"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = body_json(res).await;
    assert_eq!(body.get("status").and_then(|v| v.as_str()), Some("ok"));

    let xray = body.get("xray").expect("missing xray field");
    assert!(xray.get("status").is_some());
    assert!(xray.get("last_ok_at").is_some());
    assert!(xray.get("last_fail_at").is_some());
    assert!(xray.get("down_since").is_some());
    assert!(xray.get("consecutive_failures").is_some());
    assert!(xray.get("recoveries_observed").is_some());
}

#[tokio::test]
async fn version_check_uses_github_and_caches_and_compares() {
    struct EnvGuard {
        repo: Option<OsString>,
        api_base: Option<OsString>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match self.repo.take() {
                    Some(v) => std::env::set_var("XP_OPS_GITHUB_REPO", v),
                    None => std::env::remove_var("XP_OPS_GITHUB_REPO"),
                }
                match self.api_base.take() {
                    Some(v) => std::env::set_var("XP_OPS_GITHUB_API_BASE_URL", v),
                    None => std::env::remove_var("XP_OPS_GITHUB_API_BASE_URL"),
                }
            }
        }
    }

    let _guard = EnvGuard {
        repo: std::env::var_os("XP_OPS_GITHUB_REPO"),
        api_base: std::env::var_os("XP_OPS_GITHUB_API_BASE_URL"),
    };

    // semver compare + caching (second call must not hit upstream)
    {
        fn parse_simple_semver(raw: &str) -> Option<(u64, u64, u64)> {
            let raw = raw.trim();
            let raw = raw
                .strip_prefix('v')
                .or_else(|| raw.strip_prefix('V'))
                .unwrap_or(raw);
            let core = raw.split(['-', '+']).next()?;
            let mut parts = core.split('.');
            let major: u64 = parts.next()?.parse().ok()?;
            let minor: u64 = parts.next()?.parse().ok()?;
            let patch: u64 = parts.next()?.parse().ok()?;
            if parts.next().is_some() {
                return None;
            }
            Some((major, minor, patch))
        }

        // This test runs in CI and also in the release workflow where XP_BUILD_VERSION can set
        // crate::version::VERSION to a prerelease like `0.2.0-rc.1`. Use a mocked tag that is
        // always strictly higher than the current core semver to keep the expectation stable.
        let latest_tag = parse_simple_semver(crate::version::VERSION)
            .map(|(major, minor, patch)| format!("v{major}.{minor}.{}", patch + 1))
            .unwrap_or_else(|| "v99.99.99".to_string());

        let github = MockServer::start().await;
        unsafe {
            std::env::set_var("XP_OPS_GITHUB_API_BASE_URL", github.uri());
            std::env::set_var("XP_OPS_GITHUB_REPO", "acme/xp");
        }

        Mock::given(method("GET"))
            .and(path("/repos/acme/xp/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tag_name": latest_tag,
                "published_at": "2026-01-31T00:00:00Z"
            })))
            .mount(&github)
            .await;

        let tmp = TempDir::new().unwrap();
        let app = app(&tmp);

        let req = Request::builder()
            .method("GET")
            .uri("/api/version/check")
            .header(header::ACCEPT, "application/json")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = body_json(res).await;
        assert_eq!(
            body.pointer("/current/package").and_then(|v| v.as_str()),
            Some(crate::version::VERSION)
        );
        assert_eq!(
            body.pointer("/latest/release_tag").and_then(|v| v.as_str()),
            Some(latest_tag.as_str())
        );
        assert_eq!(body.get("has_update").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            body.get("compare_reason").and_then(|v| v.as_str()),
            Some("semver")
        );
        assert_eq!(
            body.pointer("/source/repo").and_then(|v| v.as_str()),
            Some("acme/xp")
        );
        assert_eq!(
            body.pointer("/source/api_base").and_then(|v| v.as_str()),
            Some(github.uri().as_str())
        );

        let req = Request::builder()
            .method("GET")
            .uri("/api/version/check")
            .header(header::ACCEPT, "application/json")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let requests = github.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/repos/acme/xp/releases/latest");
    }

    // uncomparable compare semantics
    {
        let github = MockServer::start().await;
        unsafe {
            std::env::set_var("XP_OPS_GITHUB_API_BASE_URL", github.uri());
            std::env::set_var("XP_OPS_GITHUB_REPO", "acme/xp2");
        }

        Mock::given(method("GET"))
            .and(path("/repos/acme/xp2/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tag_name": "main",
                "published_at": "2026-01-31T00:00:00Z"
            })))
            .mount(&github)
            .await;

        let tmp = TempDir::new().unwrap();
        let app = app(&tmp);

        let req = Request::builder()
            .method("GET")
            .uri("/api/version/check")
            .header(header::ACCEPT, "application/json")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = body_json(res).await;
        assert!(body.get("has_update").is_some());
        assert!(body.get("has_update").unwrap().is_null());
        assert_eq!(
            body.get("compare_reason").and_then(|v| v.as_str()),
            Some("uncomparable")
        );
    }

    // upstream parse failure => 502
    {
        let github = MockServer::start().await;
        unsafe {
            std::env::set_var("XP_OPS_GITHUB_API_BASE_URL", github.uri());
            std::env::set_var("XP_OPS_GITHUB_REPO", "acme/xp3");
        }

        Mock::given(method("GET"))
            .and(path("/repos/acme/xp3/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "published_at": "2026-01-31T00:00:00Z"
            })))
            .mount(&github)
            .await;

        let tmp = TempDir::new().unwrap();
        let app = app(&tmp);

        let req = Request::builder()
            .method("GET")
            .uri("/api/version/check")
            .header(header::ACCEPT, "application/json")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    }
}

fn leader_raft(
    store: Arc<Mutex<JsonSnapshotStore>>,
    cluster: &ClusterMetadata,
) -> Arc<dyn crate::raft::app::RaftFacade> {
    let raft_id = raft_node_id_from_ulid(&cluster.node_id).unwrap();
    let mut metrics = openraft::RaftMetrics::new_initial(raft_id);
    metrics.current_term = 1;
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(raft_id);
    let mut nodes = std::collections::BTreeMap::new();
    nodes.insert(
        raft_id,
        RaftNodeMeta {
            name: cluster.node_name.clone(),
            api_base_url: cluster.api_base_url.clone(),
            raft_endpoint: cluster.api_base_url.clone(),
        },
    );
    let membership =
        openraft::Membership::new(vec![std::collections::BTreeSet::from([raft_id])], nodes);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(None, membership));
    let (_tx, rx) = watch::channel(metrics);
    Arc::new(LocalRaft::new(store, rx))
}

fn app(tmp: &TempDir) -> axum::Router {
    app_with(tmp, ReconcileHandle::noop()).0
}

fn drain_reconcile_requests(
    rx: &mut mpsc::UnboundedReceiver<ReconcileRequest>,
) -> Vec<ReconcileRequest> {
    let mut out = Vec::new();
    while let Ok(req) = rx.try_recv() {
        out.push(req);
    }
    out
}

fn req(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn req_authed(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, "Bearer testtoken")
        .body(Body::empty())
        .unwrap()
}

fn req_authed_json(method: &str, uri: &str, value: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, "Bearer testtoken")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&value).unwrap()))
        .unwrap()
}

async fn record_inbound_ip_usage_samples(
    store: &Arc<Mutex<JsonSnapshotStore>>,
    minute: chrono::DateTime<chrono::Utc>,
    online_stats_unavailable: bool,
    samples: Vec<crate::inbound_ip_usage::InboundIpMinuteSample>,
) {
    let mut store = store.lock().await;
    let geo_resolver = crate::inbound_ip_usage::GeoResolver::new(None, None);
    store
        .record_inbound_ip_usage_samples(
            minute,
            crate::inbound_ip_usage::PersistedInboundIpUsageGeoDb::default(),
            online_stats_unavailable,
            &samples,
            &geo_resolver,
        )
        .unwrap();
}

fn extract_asset_paths_from_index_html(html: &str) -> Vec<String> {
    let mut out = Vec::new();

    for needle in ["src=\"/assets/", "href=\"/assets/"] {
        let mut rest = html;
        while let Some(start) = rest.find(needle) {
            let after = &rest[start + needle.len()..];
            let Some(end) = after.find('"') else {
                break;
            };
            out.push(format!("/assets/{}", &after[..end]));
            rest = &after[end..];
        }
    }

    out.sort();
    out.dedup();
    out
}

async fn body_bytes(res: axum::response::Response) -> Bytes {
    res.into_body().collect().await.unwrap().to_bytes()
}

async fn body_json(res: axum::response::Response) -> Value {
    let bytes = body_bytes(res).await;
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_text(res: axum::response::Response) -> String {
    let bytes = body_bytes(res).await;
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn warning_codes(value: &Value) -> Vec<String> {
    let mut out = value
        .as_array()
        .expect("warnings array")
        .iter()
        .filter_map(|item| item.get("code").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    out.sort();
    out
}

fn series_count_at(series: &Value, minute: &str) -> u64 {
    series
        .as_array()
        .expect("series array")
        .iter()
        .find(|item| item.get("minute").and_then(Value::as_str) == Some(minute))
        .and_then(|item| item.get("count").and_then(Value::as_u64))
        .unwrap_or_default()
}

async fn set_bootstrap_node_access_host(store: &Arc<Mutex<JsonSnapshotStore>>, access_host: &str) {
    let mut store = store.lock().await;
    let node_id = store
        .state()
        .nodes
        .keys()
        .next()
        .cloned()
        .expect("expected a bootstrap node");
    store
        .state_mut()
        .nodes
        .get_mut(&node_id)
        .unwrap()
        .access_host = access_host.to_string();
    store.save().unwrap();
}

#[tokio::test]
async fn ui_serves_index_at_root_and_embedded_assets() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);

    let res = app.clone().oneshot(req("GET", "/")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(content_type.starts_with("text/html"));
    assert!(res.headers().contains_key("content-security-policy"));

    let html = body_text(res).await;
    let assets = extract_asset_paths_from_index_html(&html);
    assert!(!assets.is_empty());

    for asset in assets {
        let res = app.clone().oneshot(req("GET", &asset)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let content_type = res
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        if asset.ends_with(".js") {
            assert_eq!(content_type, "text/javascript; charset=utf-8");
        } else if asset.ends_with(".css") {
            assert_eq!(content_type, "text/css; charset=utf-8");
        }
    }
}

#[tokio::test]
async fn ui_serves_favicon_and_manifest() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);

    let cases = [
        ("/favicon.ico", "image/x-icon"),
        ("/favicon-16x16.png", "image/png"),
        ("/favicon-32x32.png", "image/png"),
        ("/apple-touch-icon.png", "image/png"),
        ("/android-chrome-192x192.png", "image/png"),
        ("/android-chrome-512x512.png", "image/png"),
        ("/xp-mark.png", "image/png"),
        (
            "/site.webmanifest",
            "application/manifest+json; charset=utf-8",
        ),
    ];

    for (path, expected_content_type) in cases {
        let res = app.clone().oneshot(req("GET", path)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "expected {path} to be served");

        let content_type = res
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            content_type, expected_content_type,
            "unexpected content-type for {path}"
        );

        let bytes = body_bytes(res).await;
        assert!(!bytes.is_empty(), "expected {path} to return a body");
    }
}

struct SubscriptionFixtures {
    subscription_token: String,
    membership_key: String,
    user_id: String,
    node_id: String,
    endpoint_id: String,
    endpoint_tag: String,
    ss2022_password: String,
}

async fn setup_subscription_fixtures(tmp: &TempDir, app: &axum::Router) -> SubscriptionFixtures {
    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .expect("cluster ca key pem");

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let user = body_json(res).await;
    let user_id = user["user_id"].as_str().unwrap().to_string();
    let subscription_token = user["subscription_token"].as_str().unwrap().to_string();
    let credential_epoch = user
        .get("credential_epoch")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let endpoint = body_json(res).await;
    let endpoint_id = endpoint["endpoint_id"].as_str().unwrap().to_string();

    let meta: Ss2022EndpointMeta =
        serde_json::from_value(endpoint["meta"].clone()).expect("ss2022 endpoint meta");

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({
              "items": [{
                "endpoint_id": endpoint_id.clone()
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    drop(res);

    // Derive SS password deterministically (per-user secret, per-endpoint full string).
    let user_psk_b64 = crate::credentials::derive_ss2022_user_psk_b64(
        &cluster_ca_key_pem,
        &user_id,
        credential_epoch,
    )
    .expect("derive ss2022 user_psk");
    let password = ss2022_password(&meta.server_psk_b64, &user_psk_b64);

    let membership_key = membership_key(&user_id, &endpoint_id);

    SubscriptionFixtures {
        subscription_token,
        membership_key,
        user_id,
        node_id: node_id.to_string(),
        endpoint_id,
        endpoint_tag: endpoint["tag"].as_str().unwrap().to_string(),
        ss2022_password: password,
    }
}

#[tokio::test]
async fn unauthorized_admin_returns_401_with_error_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app.oneshot(req("GET", "/api/admin/nodes")).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "unauthorized");
    assert!(!json["error"]["message"].as_str().unwrap().is_empty());
    assert!(json["error"]["details"].is_object());
}

#[tokio::test]
async fn login_token_jwt_can_access_admin_endpoints() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let meta = ClusterMetadata::load(tmp.path()).unwrap();

    let token_id = crate::id::new_ulid_string();
    let now = chrono::Utc::now();
    let secret = test_admin_token_hash();
    let jwt = crate::login_token::issue_login_token_jwt(&meta.cluster_id, &token_id, now, &secret);

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/admin/alerts")
                .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn expired_login_token_jwt_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let meta = ClusterMetadata::load(tmp.path()).unwrap();

    let token_id = crate::id::new_ulid_string();
    let now = chrono::Utc::now();
    let issued_at =
        now - chrono::Duration::seconds(crate::login_token::LOGIN_TOKEN_TTL_SECONDS + 1);
    let secret = test_admin_token_hash();
    let jwt =
        crate::login_token::issue_login_token_jwt(&meta.cluster_id, &token_id, issued_at, &secret);

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/admin/alerts")
                .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn internal_client_write_requires_admin_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .oneshot(req("POST", "/api/admin/_internal/raft/client-write"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn patch_node_rejects_node_meta_but_allows_quota_reset() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let meta = ClusterMetadata::load(tmp.path()).unwrap();

    let uri = format!("/api/admin/nodes/{}", meta.node_id);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &uri,
            json!({ "node_name": "evil" }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");

    let res = app.clone().oneshot(req_authed("GET", &uri)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["node_name"], "node-1");

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &uri,
            json!({ "quota_reset": { "policy": "unlimited" } }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["quota_reset"]["policy"], "unlimited");
}

#[tokio::test]
async fn delete_node_removes_from_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let node = Node {
        node_id: new_ulid_string(),
        node_name: "extra-node".to_string(),
        access_host: "".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    {
        let mut store = store.lock().await;
        store.upsert_node(node.clone()).unwrap();
    }

    let uri = format!("/api/admin/nodes/{}", node.node_id);
    let res = app
        .clone()
        .oneshot(req_authed("DELETE", &uri))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let items = json["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .all(|n| n["node_id"].as_str().unwrap() != node.node_id)
    );
}

#[tokio::test]
async fn delete_node_rejects_if_endpoints_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let node = Node {
        node_id: new_ulid_string(),
        node_name: "extra-node".to_string(),
        access_host: "".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    {
        let mut store = store.lock().await;
        store.upsert_node(node.clone()).unwrap();
    }

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node.node_id.clone(),
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let uri = format!("/api/admin/nodes/{}", node.node_id);
    let res = app
        .clone()
        .oneshot(req_authed("DELETE", &uri))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "conflict");

    let res = app.clone().oneshot(req_authed("GET", &uri)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn delete_node_rejects_local_node() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let meta = ClusterMetadata::load(tmp.path()).unwrap();

    let uri = format!("/api/admin/nodes/{}", meta.node_id);
    let res = app
        .clone()
        .oneshot(req_authed("DELETE", &uri))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn cluster_info_is_single_node_leader_and_ids_present() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app.oneshot(req("GET", "/api/cluster/info")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;

    let node_id = json["node_id"].as_str().unwrap();
    let cluster_id = json["cluster_id"].as_str().unwrap();
    let meta = ClusterMetadata::load(tmp.path()).unwrap();
    assert!(is_ulid_string(node_id));
    assert_eq!(cluster_id, meta.cluster_id);
    assert_eq!(node_id, meta.node_id);
    assert_eq!(json["role"], "leader");
    assert_eq!(json["term"], 1);
    assert_eq!(json["leader_api_base_url"], meta.api_base_url);
}

#[tokio::test]
async fn join_token_endpoint_returns_decodable_token() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/cluster/join-tokens",
            json!({ "ttl_seconds": 900 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let token = json["join_token"].as_str().unwrap();

    let decoded =
        crate::cluster_identity::JoinToken::decode_and_validate(token, chrono::Utc::now()).unwrap();
    let meta = ClusterMetadata::load(tmp.path()).unwrap();
    let ca_key_pem = meta.read_cluster_ca_key_pem(tmp.path()).unwrap().unwrap();
    decoded.validate_one_time_secret(&ca_key_pem).unwrap();
    assert_eq!(decoded.cluster_id, meta.cluster_id);
    assert_eq!(decoded.leader_api_base_url, meta.api_base_url);
}

#[tokio::test]
async fn cluster_join_returns_cluster_ca_key_pem_when_leader_has_it() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/cluster/join-tokens",
            json!({ "ttl_seconds": 900 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let join_token = json["join_token"].as_str().unwrap().to_string();

    let decoded =
        crate::cluster_identity::JoinToken::decode_and_validate(&join_token, chrono::Utc::now())
            .unwrap();
    let expected_node_id = decoded.token_id.clone();
    let csr = crate::cluster_identity::generate_node_keypair_and_csr(&expected_node_id).unwrap();

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/cluster/join")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "join_token": join_token,
                        "node_name": "node-2",
                        "access_host": "example.com",
                        "api_base_url": "https://node-2.internal:8443",
                        "csr_pem": csr.csr_pem,
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;

    assert_eq!(json["node_id"], expected_node_id);
    assert!(!json["signed_cert_pem"].as_str().unwrap().is_empty());
    assert!(!json["cluster_ca_pem"].as_str().unwrap().is_empty());

    let key_pem = json["cluster_ca_key_pem"].as_str().unwrap();
    assert!(key_pem.starts_with("-----BEGIN"));

    let meta = ClusterMetadata::load(tmp.path()).unwrap();
    let expected_key_pem = meta
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .expect("expected bootstrap node to have a CA key");

    let got_hash = hex::encode(Sha256::digest(key_pem.as_bytes()));
    let expected_hash = hex::encode(Sha256::digest(expected_key_pem.as_bytes()));
    assert_eq!(got_hash, expected_hash);
}

#[tokio::test]
async fn follower_admin_write_does_not_redirect() {
    let tmp = tempfile::tempdir().unwrap();

    let config = test_config(tmp.path().to_path_buf());
    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(tmp.path()).unwrap();

    let store =
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
            .unwrap();
    let store = Arc::new(Mutex::new(store));

    let follower_id = raft_node_id_from_ulid(&cluster.node_id).unwrap();
    let leader_id = follower_id.wrapping_add(1);
    let mut metrics = openraft::RaftMetrics::new_initial(follower_id);
    metrics.current_term = 1;
    metrics.state = openraft::ServerState::Follower;
    metrics.current_leader = Some(leader_id);
    let mut nodes = std::collections::BTreeMap::new();
    nodes.insert(
        leader_id,
        RaftNodeMeta {
            name: "leader".to_string(),
            api_base_url: "https://leader.example.com".to_string(),
            raft_endpoint: "https://leader.example.com".to_string(),
        },
    );
    let membership =
        openraft::Membership::new(vec![std::collections::BTreeSet::from([leader_id])], nodes);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(None, membership));
    let (_tx, rx) = watch::channel(metrics);
    let raft: Arc<dyn crate::raft::app::RaftFacade> = Arc::new(LocalRaft::new(store.clone(), rx));

    let xray_health = XrayHealthHandle::new_unknown();
    let cloudflared_health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
    let (node_runtime, _node_runtime_task) = crate::node_runtime::spawn_node_runtime_monitor(
        Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let geo_db_update = test_geo_db_update_handle(&config, store.clone());
    let app = build_router(
        config,
        store,
        ReconcileHandle::noop(),
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
        geo_db_update,
    );

    let res = app
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert!(!json["user_id"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn create_user_then_list_contains_it() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let create = req_authed_json(
        "POST",
        "/api/admin/users",
        json!({
          "display_name": "alice"
        }),
    );
    let res = app.clone().oneshot(create).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let user_id = created["user_id"].as_str().unwrap().to_string();

    let res = app
        .oneshot(req_authed("GET", "/api/admin/users"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    let items = listed["items"].as_array().unwrap();
    assert!(items.iter().any(|u| u["user_id"] == user_id));
}

#[tokio::test]
async fn nodes_runtime_list_marks_unreachable_remote_nodes_as_partial() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    {
        let mut store = store.lock().await;
        store
            .upsert_node(Node {
                node_id: "01J0000000000000000000000AB".to_string(),
                node_name: "remote-a".to_string(),
                access_host: "remote-a.example.com".to_string(),
                api_base_url: "https://127.0.0.1:1".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            })
            .unwrap();
    }

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes/runtime"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = body_json(res).await;
    assert_eq!(body["partial"], Value::Bool(true));
    let unreachable = body["unreachable_nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        unreachable
            .iter()
            .any(|value| { value.as_str() == Some("01J0000000000000000000000AB") })
    );
}

#[tokio::test]
async fn node_runtime_detail_contains_components_slots_and_events() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    let node_id = listed["items"][0]["node_id"]
        .as_str()
        .expect("node id")
        .to_string();

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/nodes/{node_id}/runtime"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = body_json(res).await;
    let components = body["components"].as_array().cloned().unwrap_or_default();
    assert!(!components.is_empty());
    assert!(components.iter().any(|item| item["component"] == "xp"));
    assert!(components.iter().any(|item| item["component"] == "xray"));
    assert!(
        components
            .iter()
            .any(|item| item["component"] == "cloudflared")
    );
    assert_eq!(
        body["recent_slots"].as_array().map(|x| x.len()),
        Some(7 * 24 * 2)
    );
    assert!(body.get("events").is_some());
}

#[tokio::test]
async fn put_user_node_quota_is_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    // Create user.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let user_id = created["user_id"].as_str().unwrap().to_string();

    // Bootstrap node exists.
    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap().to_string();

    // Deprecated: static per-user node quotas are no longer editable.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/node-quotas/{node_id}"),
            json!({
              "quota_limit_bytes": 456
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::GONE);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "gone");
}

#[tokio::test]
async fn put_user_node_weight_then_list_returns_it() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    // Create user.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let user_id = created["user_id"].as_str().unwrap().to_string();

    // Bootstrap node exists.
    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap().to_string();

    // Put node weight.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/node-weights/{node_id}"),
            json!({
              "weight": 200
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["node_id"], node_id);
    assert_eq!(updated["weight"], 200);

    // List should contain the updated weight.
    let res = app
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/node-weights"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let items = json["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|i| i["node_id"] == node_id && i["weight"] == 200)
    );
}

#[tokio::test]
async fn quota_policy_node_weight_rows_supports_implicit_zero_and_explicit_weight() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    // Create user.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let user_id = created["user_id"].as_str().unwrap().to_string();

    // Bootstrap node exists.
    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap().to_string();

    // Create endpoint on node.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8488
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let endpoint = body_json(res).await;
    let endpoint_id = endpoint["endpoint_id"].as_str().unwrap().to_string();

    // Grant access (membership-only hard cut).
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({
              "items": [{
                "endpoint_id": endpoint_id.clone()
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Missing stored weight should be surfaced as implicit_zero/editor_weight=0.
    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/quota-policy/nodes/{node_id}/weight-rows"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let rows = body_json(res).await;
    let items = rows["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["user_id"], user_id);
    assert_eq!(items[0]["source"], "implicit_zero");
    assert_eq!(items[0]["editor_weight"], 0);
    assert!(items[0].get("stored_weight").is_none());
    assert_eq!(items[0]["endpoint_ids"][0], endpoint_id);

    // Persist explicit weight and ensure readback is explicit.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/node-weights/{node_id}"),
            json!({ "weight": 4321 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/quota-policy/nodes/{node_id}/weight-rows"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let rows = body_json(res).await;
    let items = rows["items"].as_array().unwrap();
    assert_eq!(items[0]["source"], "explicit");
    assert_eq!(items[0]["stored_weight"], 4321);
    assert_eq!(items[0]["editor_weight"], 4321);
}

#[tokio::test]
async fn quota_policy_node_weight_rows_requires_admin_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .oneshot(req(
            "GET",
            "/api/admin/quota-policy/nodes/node-1/weight-rows",
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn quota_policy_global_weight_rows_supports_implicit_default_and_explicit_weight() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let user_id = created["user_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            "/api/admin/quota-policy/global-weight-rows",
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let rows = body_json(res).await;
    let items = rows["items"].as_array().unwrap();
    let row = items
        .iter()
        .find(|item| item["user_id"] == user_id)
        .expect("global row must exist for created user");
    assert_eq!(row["source"], "implicit_default");
    assert_eq!(row["editor_weight"], 100);
    assert!(row.get("stored_weight").is_none());

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/quota-policy/global-weight-rows/{user_id}"),
            json!({ "weight": 4321 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .oneshot(req_authed(
            "GET",
            "/api/admin/quota-policy/global-weight-rows",
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let rows = body_json(res).await;
    let items = rows["items"].as_array().unwrap();
    let row = items
        .iter()
        .find(|item| item["user_id"] == user_id)
        .expect("global row must exist for created user");
    assert_eq!(row["source"], "explicit");
    assert_eq!(row["stored_weight"], 4321);
    assert_eq!(row["editor_weight"], 4321);
}

#[tokio::test]
async fn quota_policy_node_policy_defaults_to_inherit_and_can_update() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/quota-policy/nodes/{node_id}/policy"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let policy = body_json(res).await;
    assert_eq!(policy["node_id"], node_id);
    assert_eq!(policy["inherit_global"], true);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/quota-policy/nodes/{node_id}/policy"),
            json!({ "inherit_global": false }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let policy = body_json(res).await;
    assert_eq!(policy["inherit_global"], false);

    let res = app
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/quota-policy/nodes/{node_id}/policy"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let policy = body_json(res).await;
    assert_eq!(policy["inherit_global"], false);
}

#[tokio::test]
async fn quota_policy_global_weight_rows_requires_admin_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .oneshot(req("GET", "/api/admin/quota-policy/global-weight-rows"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_endpoint_then_list_contains_it() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let create = req_authed_json(
        "POST",
        "/api/admin/endpoints",
        json!({
          "node_id": node_id,
          "kind": "ss2022_2022_blake3_aes_128_gcm",
          "port": 8388
        }),
    );
    let res = app.clone().oneshot(create).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap().to_string();

    let res = app
        .oneshot(req_authed("GET", "/api/admin/endpoints"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    let items = listed["items"].as_array().unwrap();
    assert!(items.iter().any(|e| e["endpoint_id"] == endpoint_id));
}

#[tokio::test]
async fn patch_admin_node_updates_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node = nodes["items"][0].as_object().unwrap();
    let node_id = node["node_id"].as_str().unwrap();
    let original_node_name = node["node_name"].as_str().unwrap();
    let original_api_base_url = node["api_base_url"].as_str().unwrap();
    let original_access_host = node["access_host"].as_str().unwrap();

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/nodes/{node_id}"),
            json!({
              "quota_reset": { "policy": "unlimited" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["node_id"], node_id);
    assert_eq!(updated["node_name"], original_node_name);
    assert_eq!(updated["access_host"], original_access_host);
    assert_eq!(updated["api_base_url"], original_api_base_url);
    assert_eq!(updated["quota_reset"]["policy"], "unlimited");
}

#[tokio::test]
async fn patch_admin_node_allows_quota_limit_bytes_update() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/nodes/{node_id}"),
            json!({
              "quota_limit_bytes": 456
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["node_id"], node_id);
    assert_eq!(updated["quota_limit_bytes"], 456);
}

#[tokio::test]
async fn patch_admin_node_rejects_quota_limit_bytes_when_reset_is_unlimited() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    // Unlimited reset is allowed when shared quota is disabled.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/nodes/{node_id}"),
            json!({
              "quota_reset": { "policy": "unlimited" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // But shared quota requires a finite cycle window.
    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/nodes/{node_id}"),
            json!({
              "quota_limit_bytes": 456
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn patch_admin_node_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let node_id = new_ulid_string();

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/nodes/{node_id}"),
            json!({
              "quota_reset": { "policy": "unlimited" }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_found");
}

#[tokio::test]
async fn admin_config_requires_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app.oneshot(req("GET", "/api/admin/config")).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn admin_config_returns_safe_view_and_masks_token() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .oneshot(req_authed("GET", "/api/admin/config"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;

    assert_eq!(json["bind"], "127.0.0.1:0");
    assert_eq!(json["xray_api_addr"], "127.0.0.1:10085");
    assert_eq!(json["node_name"], "node-1");
    assert_eq!(json["access_host"], "");
    assert_eq!(json["api_base_url"], "https://127.0.0.1:62416");
    assert_eq!(json["quota_poll_interval_secs"], 10);
    assert_eq!(json["quota_auto_unban"], true);

    assert_eq!(json["admin_token_present"], true);
    assert_eq!(json["admin_token_masked"], "********");
    assert_ne!(json["admin_token_masked"], "testtoken");
}

#[tokio::test]
async fn patch_admin_user_updates_fields_preserves_token() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let user_id = created["user_id"].as_str().unwrap().to_string();
    let token = created["subscription_token"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/users/{user_id}"),
            json!({
              "display_name": "alice-2"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["user_id"], user_id);
    assert_eq!(updated["display_name"], "alice-2");
    assert_eq!(updated["quota_reset"]["policy"], "monthly");
    assert_eq!(updated["quota_reset"]["day_of_month"], 1);
    assert_eq!(updated["quota_reset"]["tz_offset_minutes"], 480);
    assert_eq!(updated["subscription_token"], token);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/users/{user_id}"),
            json!({
              "quota_reset": {
                "policy": "monthly",
                "day_of_month": 15,
                "tz_offset_minutes": 0
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["display_name"], "alice-2");
    assert_eq!(updated["quota_reset"]["policy"], "monthly");
    assert_eq!(updated["quota_reset"]["day_of_month"], 15);
    assert_eq!(updated["quota_reset"]["tz_offset_minutes"], 0);
}

#[tokio::test]
async fn patch_admin_user_allows_priority_tier_update() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let user_id = created["user_id"].as_str().unwrap().to_string();

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/users/{user_id}"),
            json!({
              "priority_tier": "p1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["user_id"], user_id);
    assert_eq!(updated["priority_tier"], "p1");
}

#[tokio::test]
async fn patch_admin_user_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let user_id = new_ulid_string();

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/users/{user_id}"),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_found");
}

#[tokio::test]
async fn patch_admin_endpoint_vless_updates_meta_and_port() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "vless_reality_vision_tcp",
              "port": 443,
              "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap().to_string();
    let reality_keys = created["meta"]["reality_keys"].clone();
    let short_ids = created["meta"]["short_ids"].clone();
    let active_short_id = created["meta"]["active_short_id"].clone();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
              "port": 8443
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["endpoint_id"], endpoint_id);
    assert_eq!(updated["port"], 8443);
    assert_eq!(updated["meta"]["reality"]["dest"], "example.com:443");
    assert_eq!(updated["meta"]["reality"]["server_names"][0], "example.com");
    assert_eq!(updated["meta"]["reality"]["fingerprint"], "chrome");
    assert_eq!(updated["meta"]["reality_keys"], reality_keys);
    assert_eq!(updated["meta"]["short_ids"], short_ids);
    assert_eq!(updated["meta"]["active_short_id"], active_short_id);
    assert_eq!(updated["meta"].get("public_domain"), None);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
              "reality": {
                "dest": "edge.example.com:443",
                "server_names": ["edge.example.com"],
                "fingerprint": "firefox"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["endpoint_id"], endpoint_id);
    assert_eq!(updated["port"], 8443);
    assert_eq!(updated["meta"]["reality"]["dest"], "edge.example.com:443");
    assert_eq!(
        updated["meta"]["reality"]["server_names"][0],
        "edge.example.com"
    );
    assert_eq!(updated["meta"]["reality"]["fingerprint"], "firefox");
    assert_eq!(updated["meta"]["reality_keys"], reality_keys);
    assert_eq!(updated["meta"]["short_ids"], short_ids);
    assert_eq!(updated["meta"]["active_short_id"], active_short_id);
    assert_eq!(updated["meta"].get("public_domain"), None);
}

#[tokio::test]
async fn patch_admin_endpoint_rejects_kind_mismatch_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap().to_string();

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
              "port": 8389,
              "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn patch_admin_endpoint_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let endpoint_id = new_ulid_string();

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_found");
}

#[tokio::test]
async fn patch_admin_endpoint_updates_node_id_preserves_meta() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let src_node_id = nodes["items"][0]["node_id"].as_str().unwrap().to_string();

    let dst_node_id = new_ulid_string();
    {
        let mut store = store.lock().await;
        store.state_mut().nodes.insert(
            dst_node_id.clone(),
            Node {
                node_id: dst_node_id.clone(),
                node_name: "node-2".to_string(),
                access_host: "node-2.example.com".to_string(),
                api_base_url: "https://node-2.example.com".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            },
        );
        store.save().unwrap();
    }

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": src_node_id,
              "kind": "vless_reality_vision_tcp",
              "port": 443,
              "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap().to_string();
    let tag = created["tag"].as_str().unwrap().to_string();
    let meta = created["meta"].clone();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
              "node_id": dst_node_id.clone()
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["endpoint_id"], endpoint_id);
    assert_eq!(updated["tag"], tag);
    assert_eq!(updated["node_id"], dst_node_id);
    assert_eq!(updated["port"], 443);
    assert_eq!(updated["meta"], meta);
}

#[tokio::test]
async fn patch_admin_endpoint_rejects_unknown_node_id() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "vless_reality_vision_tcp",
              "port": 443,
              "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap().to_string();

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
              "node_id": new_ulid_string()
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn patch_admin_endpoint_rejects_port_conflict_on_target_node() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let src_node_id = nodes["items"][0]["node_id"].as_str().unwrap().to_string();

    let dst_node_id = new_ulid_string();
    {
        let mut store = store.lock().await;
        store.state_mut().nodes.insert(
            dst_node_id.clone(),
            Node {
                node_id: dst_node_id.clone(),
                node_name: "node-2".to_string(),
                access_host: "node-2.example.com".to_string(),
                api_base_url: "https://node-2.example.com".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            },
        );
        store.save().unwrap();
    }

    // Create an endpoint on the target node that reserves port 443.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": dst_node_id.clone(),
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 443
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Create another endpoint on the source node, also using port 443.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": src_node_id,
              "kind": "vless_reality_vision_tcp",
              "port": 443,
              "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap().to_string();

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
              "node_id": dst_node_id.clone()
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "conflict");
}

#[tokio::test]
async fn put_user_access_with_missing_resources_returns_404_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    // Missing user.
    let missing_user_id = new_ulid_string();
    let create = req_authed_json(
        "PUT",
        &format!("/api/admin/users/{missing_user_id}/access"),
        json!({ "items": [] }),
    );
    let res = app.clone().oneshot(create).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_found");

    // Missing endpoint.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({ "display_name": "alice" }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let user = body_json(res).await;
    let user_id = user["user_id"].as_str().unwrap();

    let create = req_authed_json(
        "PUT",
        &format!("/api/admin/users/{user_id}/access"),
        json!({
          "items": [{
            "endpoint_id": new_ulid_string()
          }]
        }),
    );
    let res = app.oneshot(create).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_found");
}

#[tokio::test]
async fn legacy_grant_groups_endpoints_return_404_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let requests = vec![
        req_authed("GET", "/api/admin/grant-groups"),
        req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "legacy",
              "members": []
            }),
        ),
        req_authed("GET", "/api/admin/grant-groups/legacy"),
        req_authed_json(
            "PUT",
            "/api/admin/grant-groups/legacy",
            json!({
              "members": []
            }),
        ),
        req_authed("DELETE", "/api/admin/grant-groups/legacy"),
    ];

    for req in requests {
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let json = body_json(res).await;
        assert_eq!(json["error"]["code"], "not_found");
    }
}

#[tokio::test]
async fn grants_endpoints_return_404_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({ "display_name": "alice" }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let user = body_json(res).await;
    let user_id = user["user_id"].as_str().unwrap();

    let requests = vec![
        req_authed("GET", "/api/admin/grants"),
        req_authed_json("POST", "/api/admin/grants", json!({})),
        req_authed("GET", "/api/admin/grants/grant_legacy"),
        req_authed("DELETE", "/api/admin/grants/grant_legacy"),
        req_authed("GET", &format!("/api/admin/users/{user_id}/grants")),
        req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/grants"),
            json!({ "items": [] }),
        ),
    ];

    for req in requests {
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let json = body_json(res).await;
        assert_eq!(json["error"]["code"], "not_found");
    }
}

#[tokio::test]
async fn put_user_access_dedups_and_allows_clear() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    let user = body_json(res).await;

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    let endpoint = body_json(res).await;
    let endpoint_id = endpoint["endpoint_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!(
                "/api/admin/users/{}/access",
                user["user_id"].as_str().unwrap()
            ),
            json!({
              "items": [{
                "endpoint_id": endpoint_id
              }, {
                "endpoint_id": endpoint_id
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let put = body_json(res).await;
    assert_eq!(put["created"], 1);
    assert_eq!(put["deleted"], 0);
    assert_eq!(put["items"].as_array().unwrap().len(), 1);
    assert_eq!(put["items"][0]["endpoint_id"], endpoint_id);

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!(
                "/api/admin/users/{}/access",
                user["user_id"].as_str().unwrap()
            ),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["items"].as_array().unwrap().len(), 1);
    assert_eq!(updated["items"][0]["endpoint_id"], endpoint_id);
    assert_eq!(updated["items"][0]["node_id"], node_id);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!(
                "/api/admin/users/{}/access",
                user["user_id"].as_str().unwrap()
            ),
            json!({ "items": [] }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .oneshot(req_authed(
            "GET",
            &format!(
                "/api/admin/users/{}/access",
                user["user_id"].as_str().unwrap()
            ),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn post_admin_endpoints_schedules_full_reconcile() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (app, _store) = app_with(&tmp, ReconcileHandle::from_sender(tx));

    let res = app
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": "node-1",
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(
        drain_reconcile_requests(&mut rx),
        vec![ReconcileRequest::Full]
    );
}

#[tokio::test]
async fn put_admin_user_access_schedules_full_reconcile() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (app, store) = app_with(&tmp, ReconcileHandle::from_sender(tx));

    let (user_id, endpoint_id) = {
        let mut store = store.lock().await;
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                "node-1".to_string(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        (user.user_id, endpoint.endpoint_id)
    };

    let res = app
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({
              "items": [{
                "endpoint_id": endpoint_id
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(
        drain_reconcile_requests(&mut rx),
        vec![ReconcileRequest::Full]
    );
}

#[tokio::test]
async fn put_admin_user_access_twice_schedules_full_reconcile() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (app, store) = app_with(&tmp, ReconcileHandle::from_sender(tx));

    let (user_id, endpoint_id) = {
        let mut store = store.lock().await;
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                "node-1".to_string(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        (user.user_id, endpoint.endpoint_id)
    };

    // First apply (and drain its reconcile request).
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({
              "items": [{
                "endpoint_id": endpoint_id
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        drain_reconcile_requests(&mut rx),
        vec![ReconcileRequest::Full]
    );

    let res = app
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({
              "items": [{
                "endpoint_id": endpoint_id
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(
        drain_reconcile_requests(&mut rx),
        vec![ReconcileRequest::Full]
    );
}

#[tokio::test]
async fn post_rotate_shortid_schedules_rebuild_inbound() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (app, store) = app_with(&tmp, ReconcileHandle::from_sender(tx));

    let endpoint_id = {
        let mut store = store.lock().await;
        let endpoint = store
            .create_endpoint(
                "node-1".to_string(),
                EndpointKind::VlessRealityVisionTcp,
                443,
                json!({
                  "reality": {
                    "dest": "example.com:443",
                    "server_names": ["example.com"],
                    "fingerprint": "chrome"
                  }
                }),
            )
            .unwrap();
        endpoint.endpoint_id
    };

    let res = app
        .oneshot(req_authed(
            "POST",
            &format!("/api/admin/endpoints/{endpoint_id}/rotate-shortid"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(
        drain_reconcile_requests(&mut rx),
        vec![ReconcileRequest::RebuildInbound { endpoint_id }]
    );
}

#[tokio::test]
async fn delete_admin_endpoint_schedules_remove_inbound_then_full() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (app, store) = app_with(&tmp, ReconcileHandle::from_sender(tx));

    let (endpoint_id, tag) = {
        let mut store = store.lock().await;
        let endpoint = store
            .create_endpoint(
                "node-1".to_string(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        (endpoint.endpoint_id, endpoint.tag)
    };

    let res = app
        .oneshot(req_authed(
            "DELETE",
            &format!("/api/admin/endpoints/{endpoint_id}"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    assert_eq!(
        drain_reconcile_requests(&mut rx),
        vec![
            ReconcileRequest::RemoveInbound { tag },
            ReconcileRequest::Full
        ]
    );
}

#[tokio::test]
async fn put_admin_user_access_empty_schedules_full_reconcile() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (app, store) = app_with(&tmp, ReconcileHandle::from_sender(tx));

    let (user_id, endpoint_id) = {
        let mut store = store.lock().await;
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                "node-1".to_string(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        (user.user_id, endpoint.endpoint_id)
    };

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({
              "items": [{
                "endpoint_id": endpoint_id
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        drain_reconcile_requests(&mut rx),
        vec![ReconcileRequest::Full]
    );

    let res = app
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({ "items": [] }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(
        drain_reconcile_requests(&mut rx),
        vec![ReconcileRequest::Full]
    );
}

#[cfg(any())]
#[tokio::test]
async fn grant_usage_includes_warning_fields() {
    #[derive(Debug, Default)]
    struct TestStatsService;

    #[tonic::async_trait]
    impl StatsService for TestStatsService {
        async fn get_stats(
            &self,
            request: tonic::Request<GetStatsRequest>,
        ) -> Result<tonic::Response<GetStatsResponse>, tonic::Status> {
            let req = request.into_inner();
            let value = if req.name.ends_with(">>>uplink") {
                100
            } else if req.name.ends_with(">>>downlink") {
                200
            } else {
                return Err(tonic::Status::not_found("missing stat"));
            };
            Ok(tonic::Response::new(GetStatsResponse {
                stat: Some(Stat {
                    name: req.name,
                    value,
                }),
            }))
        }

        async fn get_stats_online(
            &self,
            _request: tonic::Request<GetStatsRequest>,
        ) -> Result<tonic::Response<GetStatsResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_stats_online"))
        }

        async fn query_stats(
            &self,
            _request: tonic::Request<QueryStatsRequest>,
        ) -> Result<tonic::Response<QueryStatsResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("query_stats"))
        }

        async fn get_sys_stats(
            &self,
            _request: tonic::Request<SysStatsRequest>,
        ) -> Result<tonic::Response<SysStatsResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_sys_stats"))
        }

        async fn get_stats_online_ip_list(
            &self,
            _request: tonic::Request<GetStatsRequest>,
        ) -> Result<tonic::Response<GetStatsOnlineIpListResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_stats_online_ip_list"))
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let xray_api_addr = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(StatsServiceServer::new(TestStatsService::default()))
            .serve_with_incoming_shutdown(incoming, async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let mut config = test_config(tmp.path().to_path_buf());
    config.xray_api_addr = xray_api_addr;

    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(tmp.path()).unwrap();

    let store =
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
            .unwrap();
    let store = Arc::new(Mutex::new(store));
    let raft = leader_raft(store.clone(), &cluster);
    let xray_health = XrayHealthHandle::new_unknown();
    let cloudflared_health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
    let (node_runtime, _node_runtime_task) = crate::node_runtime::spawn_node_runtime_monitor(
        Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let app = build_router(
        config,
        store,
        ReconcileHandle::noop(),
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
        geo_db_update,
    );

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice",
              "cycle_policy_default": "by_user",
              "cycle_day_of_month_default": 1
            }),
        ))
        .await
        .unwrap();
    let user = body_json(res).await;

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    let endpoint = body_json(res).await;

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grants",
            json!({
              "user_id": user["user_id"],
              "endpoint_id": endpoint["endpoint_id"],
              "quota_limit_bytes": 0,
              "cycle_policy": "inherit_user",
              "cycle_day_of_month": null,
              "note": null
            }),
        ))
        .await
        .unwrap();
    let grant = body_json(res).await;
    let grant_id = grant["grant_id"].as_str().unwrap();

    let res = app
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/grants/{grant_id}/usage"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["grant_id"], grant_id);
    assert_eq!(json["used_bytes"], 300);
    let start = json["cycle_start_at"].as_str().unwrap();
    let end = json["cycle_end_at"].as_str().unwrap();
    let start = chrono::DateTime::parse_from_rfc3339(start).unwrap();
    let end = chrono::DateTime::parse_from_rfc3339(end).unwrap();
    assert!(end > start);
    assert_eq!(json["owner_node_id"], node_id);
    assert_eq!(json["desired_enabled"], true);
    assert_eq!(json["quota_banned"], false);
    assert!(json["quota_banned_at"].is_null());
    assert_eq!(json["effective_enabled"], true);
    assert!(json["warning"].is_null());

    let _ = shutdown_tx.send(());
}

#[cfg(any())]
#[tokio::test]
async fn grant_usage_warns_on_quota_mismatch() {
    #[derive(Debug, Default)]
    struct TestStatsService;

    #[tonic::async_trait]
    impl StatsService for TestStatsService {
        async fn get_stats(
            &self,
            request: tonic::Request<GetStatsRequest>,
        ) -> Result<tonic::Response<GetStatsResponse>, tonic::Status> {
            let req = request.into_inner();
            let value = if req.name.ends_with(">>>uplink") {
                100
            } else if req.name.ends_with(">>>downlink") {
                200
            } else {
                return Err(tonic::Status::not_found("missing stat"));
            };
            Ok(tonic::Response::new(GetStatsResponse {
                stat: Some(Stat {
                    name: req.name,
                    value,
                }),
            }))
        }

        async fn get_stats_online(
            &self,
            _request: tonic::Request<GetStatsRequest>,
        ) -> Result<tonic::Response<GetStatsResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_stats_online"))
        }

        async fn query_stats(
            &self,
            _request: tonic::Request<QueryStatsRequest>,
        ) -> Result<tonic::Response<QueryStatsResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("query_stats"))
        }

        async fn get_sys_stats(
            &self,
            _request: tonic::Request<SysStatsRequest>,
        ) -> Result<tonic::Response<SysStatsResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_sys_stats"))
        }

        async fn get_stats_online_ip_list(
            &self,
            _request: tonic::Request<GetStatsRequest>,
        ) -> Result<tonic::Response<GetStatsOnlineIpListResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_stats_online_ip_list"))
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let xray_api_addr = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(StatsServiceServer::new(TestStatsService::default()))
            .serve_with_incoming_shutdown(incoming, async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let mut config = test_config(tmp.path().to_path_buf());
    config.xray_api_addr = xray_api_addr;

    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(tmp.path()).unwrap();

    let store =
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
            .unwrap();
    let store = Arc::new(Mutex::new(store));
    let raft = leader_raft(store.clone(), &cluster);
    let xray_health = XrayHealthHandle::new_unknown();
    let cloudflared_health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
    let (node_runtime, _node_runtime_task) = crate::node_runtime::spawn_node_runtime_monitor(
        Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let geo_db_update = test_geo_db_update_handle(&config, store.clone());
    let app = build_router(
        config,
        store.clone(),
        ReconcileHandle::noop(),
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
        geo_db_update,
    );

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice",
              "cycle_policy_default": "by_user",
              "cycle_day_of_month_default": 1
            }),
        ))
        .await
        .unwrap();
    let user = body_json(res).await;

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    let endpoint = body_json(res).await;

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grants",
            json!({
              "user_id": user["user_id"],
              "endpoint_id": endpoint["endpoint_id"],
              "quota_limit_bytes": 0,
              "cycle_policy": "inherit_user",
              "cycle_day_of_month": null,
              "note": null
            }),
        ))
        .await
        .unwrap();
    let grant = body_json(res).await;
    let grant_id = grant["grant_id"].as_str().unwrap();

    let banned_at = "2025-12-18T00:00:00Z".to_string();
    {
        let mut store = store.lock().await;
        store.set_quota_banned(grant_id, banned_at.clone()).unwrap();
    }

    let res = app
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/grants/{grant_id}/usage"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["grant_id"], grant_id);
    assert_eq!(json["desired_enabled"], true);
    assert_eq!(json["quota_banned"], true);
    assert_eq!(json["quota_banned_at"], banned_at);
    assert_eq!(json["effective_enabled"], false);
    assert_eq!(
        json["warning"],
        "quota enforced on owner node but desired state is still enabled"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn subscription_endpoint_does_not_require_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let token = setup_subscription_fixtures(&tmp, &app)
        .await
        .subscription_token;

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=raw")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
}

#[tokio::test]
async fn subscription_default_base64_decodes_to_subscription_text_and_content_type() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let token = setup_subscription_fixtures(&tmp, &app)
        .await
        .subscription_token;

    let res = app
        .clone()
        .oneshot(req("GET", &format!("/api/sub/{token}")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
    let base64_body = body_text(res).await;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(base64_body.trim())
        .unwrap();
    let decoded_text = String::from_utf8(decoded).unwrap();
    assert!(decoded_text.ends_with('\n'));
    assert!(
        decoded_text.contains("ss://") || decoded_text.contains("vless://"),
        "expected decoded subscription text to contain at least one proxy uri"
    );

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=raw")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
    let raw_body = body_text(res).await;

    assert!(raw_body.ends_with('\n'));
    assert!(
        raw_body.contains("ss://") || raw_body.contains("vless://"),
        "expected raw subscription text to contain at least one proxy uri"
    );
}

#[tokio::test]
async fn subscription_format_clash_returns_yaml_with_proxies() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let token = setup_subscription_fixtures(&tmp, &app)
        .await
        .subscription_token;

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=clash")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/yaml; charset=utf-8"
    );
    let body = body_text(res).await;
    let yaml: YamlValue = serde_yaml::from_str(&body).unwrap();

    let proxies = yaml.get("proxies").and_then(|v| v.as_sequence()).unwrap();
    assert!(!proxies.is_empty());

    let first = proxies[0].as_mapping().unwrap();
    assert!(first.contains_key("server"));
    assert!(first.contains_key("port"));
    assert!(
        first.contains_key("password") || first.contains_key("uuid"),
        "expected ss2022 or vless-like fields"
    );
}

#[tokio::test]
async fn subscription_format_mihomo_without_profile_falls_back_to_clash_yaml() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let token = setup_subscription_fixtures(&tmp, &app)
        .await
        .subscription_token;

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=mihomo")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/yaml; charset=utf-8"
    );
    let body = body_text(res).await;
    let yaml: YamlValue = serde_yaml::from_str(&body).unwrap();
    let proxies = yaml.get("proxies").and_then(|v| v.as_sequence()).unwrap();
    assert!(!proxies.is_empty());
}

#[tokio::test]
async fn admin_user_mihomo_profile_roundtrip_and_subscription_rendering() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id.clone();
    let token = fixtures.subscription_token;

    let put_res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
            json!({
              "mixin_yaml": r#"port: 0
proxy-groups:
  - name: "🛣️ JP/HK/TW"
    type: url-test
    use: []
rules: []
"#,
              "extra_proxies_yaml": r#"- name: "custom-direct"
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
- name: "custom-JP"
  type: ss
  server: japan.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "jp:def"
  udp: true
"#,
              "extra_proxy_providers_yaml": r#"providerA:
  type: http
  path: ./provider-a.yaml
  url: https://example.com/sub-a
"#,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(put_res.status(), StatusCode::OK);

    let get_res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
        ))
        .await
        .unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);
    let profile = body_json(get_res).await;
    assert_eq!(
        profile["mixin_yaml"].as_str().unwrap().contains("port: 0"),
        true
    );
    assert_eq!(
        profile["extra_proxy_providers_yaml"]
            .as_str()
            .unwrap()
            .contains("providerA"),
        true
    );
    assert!(
        profile.get("template_yaml").is_none(),
        "response should only expose mixin_yaml"
    );

    let sub_res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=mihomo")))
        .await
        .unwrap();
    assert_eq!(sub_res.status(), StatusCode::OK);
    let sub_text = body_text(sub_res).await;
    let yaml: YamlValue = serde_yaml::from_str(&sub_text).unwrap();

    let providers = yaml
        .get("proxy-providers")
        .and_then(YamlValue::as_mapping)
        .expect("proxy-providers must exist");
    assert!(providers.contains_key("providerA"));

    let groups = yaml
        .get("proxy-groups")
        .and_then(YamlValue::as_sequence)
        .expect("proxy-groups must exist");
    let outer_group = groups
        .iter()
        .find(|g| g.get("name").and_then(YamlValue::as_str) == Some("🛣️ JP/HK/TW"))
        .expect("outer group missing");
    let outer_use = outer_group
        .get("use")
        .and_then(YamlValue::as_sequence)
        .expect("outer group use missing")
        .iter()
        .filter_map(YamlValue::as_str)
        .collect::<Vec<_>>();
    assert!(outer_use.contains(&"providerA"));

    for expected in [
        "🛣️ Japan",
        "🌟 Japan",
        "🔒 Japan",
        "🤯 Japan",
        "🛣️ HongKong",
        "🌟 HongKong",
        "🔒 HongKong",
        "🤯 HongKong",
        "🛣️ Taiwan",
        "🌟 Taiwan",
        "🔒 Taiwan",
        "🤯 Taiwan",
        "🛣️ Korea",
        "🌟 Korea",
        "🔒 Korea",
        "🤯 Korea",
    ] {
        assert!(
            groups
                .iter()
                .any(|g| g.get("name").and_then(YamlValue::as_str) == Some(expected)),
            "compat region group missing from output: {expected}"
        );
    }

    let proxies = yaml
        .get("proxies")
        .and_then(YamlValue::as_sequence)
        .expect("proxies must exist");
    assert!(
        proxies.iter().any(|item| {
            item.get("name")
                .and_then(YamlValue::as_str)
                .map(|n| n.ends_with("-chain"))
                .unwrap_or(false)
                && item.get("dialer-proxy").and_then(YamlValue::as_str) == Some("🛣️ JP/HK/TW")
        }),
        "expected at least one generated single-chain proxy"
    );

    let base = proxies
        .iter()
        .filter_map(|p| p.get("name").and_then(YamlValue::as_str))
        .find_map(|name| name.strip_suffix("-ss"))
        .expect("expected at least one generated -ss proxy for landing group test");
    let landing_group_name = format!("🛬 {base}");

    let landing_group = groups
        .iter()
        .find(|g| g.get("name").and_then(YamlValue::as_str) == Some(&landing_group_name))
        .expect("expected per-base landing group to exist");
    let landing_proxies = landing_group
        .get("proxies")
        .and_then(YamlValue::as_sequence)
        .expect("landing group proxies missing")
        .iter()
        .filter_map(YamlValue::as_str)
        .collect::<Vec<_>>();
    let expected_chain = format!("{base}-chain");
    let expected_ss = format!("{base}-ss");
    assert_eq!(
        landing_proxies,
        vec![expected_chain.as_str(), expected_ss.as_str()]
    );

    let landing_pool = groups
        .iter()
        .find(|g| g.get("name").and_then(YamlValue::as_str) == Some("🔒 落地"))
        .expect("expected built-in landing pool group 🔒 落地");
    let landing_pool_proxies = landing_pool
        .get("proxies")
        .and_then(YamlValue::as_sequence)
        .expect("landing pool proxies missing")
        .iter()
        .filter_map(YamlValue::as_str)
        .collect::<Vec<_>>();
    assert!(
        landing_pool_proxies.contains(&landing_group_name.as_str()),
        "expected 🔒 落地 to include per-base landing group"
    );
}

#[tokio::test]
async fn admin_user_mihomo_profile_rendering_without_proxy_providers_still_works() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id.clone();
    let token = fixtures.subscription_token;

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
            json!({
              "mixin_yaml": "port: 0
rules: []
",
              "extra_proxies_yaml": "",
              "extra_proxy_providers_yaml": "",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=mihomo")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let sub_text = body_text(res).await;
    let yaml: YamlValue = serde_yaml::from_str(&sub_text).unwrap();

    let providers = yaml
        .get("proxy-providers")
        .and_then(YamlValue::as_mapping)
        .expect("proxy-providers must exist");
    assert!(
        providers.is_empty(),
        "proxy-providers should be empty when omitted"
    );

    let groups = yaml
        .get("proxy-groups")
        .and_then(YamlValue::as_sequence)
        .expect("proxy-groups must exist");
    let outer_group = groups
        .iter()
        .find(|g| g.get("name").and_then(YamlValue::as_str) == Some("🛣️ JP/HK/TW"))
        .expect("expected built-in outer group 🛣️ JP/HK/TW");
    let use_values = outer_group
        .get("use")
        .and_then(YamlValue::as_sequence)
        .expect("outer group use missing");
    assert!(
        use_values.is_empty(),
        "outer group use should stay empty without providers"
    );
    let fallback_values = outer_group
        .get("proxies")
        .and_then(YamlValue::as_sequence)
        .expect("outer group should fall back to DIRECT without providers");
    assert_eq!(
        fallback_values,
        &vec![YamlValue::String("DIRECT".to_string())]
    );
}

#[tokio::test]
async fn admin_user_mihomo_profile_put_autosplits_full_config_template() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id;
    let token = fixtures.subscription_token;

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
            json!({
              "mixin_yaml": r#"port: 0
proxies:
  - name: "custom-direct"
    type: ss
    server: custom.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: "abc:def"
    udp: true
proxy-providers:
  providerA:
    type: http
    path: ./provider-a.yaml
    url: https://example.com/sub-a
proxy-groups:
  - name: "Auto"
    type: select
    use: ["providerA"]
    proxies: ["DIRECT", "🛣️ Japan", "🔒 Japan"]
  - name: "🛣️ Japan"
    type: url-test
    use: ["providerA"]
  - name: "🔒 Japan"
    type: fallback
    proxies: ["🛣️ Japan"]
rules: []
"#,
              "extra_proxies_yaml": "",
              "extra_proxy_providers_yaml": "",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let profile = body_json(res).await;

    let mixin_yaml = profile["mixin_yaml"].as_str().unwrap();
    let template_root: YamlValue = serde_yaml::from_str(mixin_yaml).unwrap();
    let template_map = template_root
        .as_mapping()
        .expect("template must be a mapping");
    assert!(
        !template_map.contains_key(YamlValue::String("proxies".to_string())),
        "expected top-level proxies to be removed from mixin_yaml"
    );
    assert!(
        !template_map.contains_key(YamlValue::String("proxy-providers".to_string())),
        "expected top-level proxy-providers to be removed from mixin_yaml"
    );

    let extra_proxies_yaml = profile["extra_proxies_yaml"].as_str().unwrap();
    let extra_proxies_root: YamlValue = serde_yaml::from_str(extra_proxies_yaml).unwrap();
    let extra_proxies = extra_proxies_root
        .as_sequence()
        .expect("extra_proxies_yaml must be a sequence");
    assert!(
        extra_proxies.iter().any(|proxy| {
            proxy
                .get("name")
                .and_then(YamlValue::as_str)
                .is_some_and(|name| name == "custom-direct")
        }),
        "expected extracted extra_proxies_yaml to include custom-direct"
    );

    let extra_providers_yaml = profile["extra_proxy_providers_yaml"].as_str().unwrap();
    let extra_providers_root: YamlValue = serde_yaml::from_str(extra_providers_yaml).unwrap();
    let extra_providers = extra_providers_root
        .as_mapping()
        .expect("extra_proxy_providers_yaml must be a mapping");
    assert!(
        extra_providers.contains_key(YamlValue::String("providerA".to_string())),
        "expected extracted extra_proxy_providers_yaml to include providerA"
    );
    assert!(
        profile.get("template_yaml").is_none(),
        "response should only expose mixin_yaml after autosplit"
    );

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=mihomo")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let sub_text = body_text(res).await;
    let yaml: YamlValue = serde_yaml::from_str(&sub_text).unwrap();

    let providers = yaml
        .get("proxy-providers")
        .and_then(YamlValue::as_mapping)
        .expect("proxy-providers must exist");
    assert!(
        providers.contains_key(YamlValue::String("providerA".to_string())),
        "expected subscription output proxy-providers to include providerA"
    );

    let proxies = yaml
        .get("proxies")
        .and_then(YamlValue::as_sequence)
        .expect("proxies must exist");
    assert!(
        proxies.iter().any(|proxy| {
            proxy
                .get("name")
                .and_then(YamlValue::as_str)
                .is_some_and(|name| name == "custom-direct")
        }),
        "expected subscription output to include custom-direct"
    );
}

#[tokio::test]
async fn admin_user_mihomo_profile_get_and_render_autosplit_legacy_stored_full_config() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id.clone();
    let token = fixtures.subscription_token;

    {
        let mut store = store.lock().await;
        store.state_mut().user_mihomo_profiles.insert(
            user_id.clone(),
            crate::state::UserMihomoProfile {
                mixin_yaml: r#"port: 0
proxies:
  - name: "custom-direct"
    type: ss
    server: custom.example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: "abc:def"
    udp: true
proxy-providers:
  providerA:
    type: http
    path: ./provider-a.yaml
    url: https://example.com/sub-a
proxy-groups:
  - name: "Auto"
    type: select
    use: ["providerA"]
    proxies: ["DIRECT"]
rules: []
"#
                .to_string(),
                extra_proxies_yaml: "".to_string(),
                extra_proxy_providers_yaml: "".to_string(),
            },
        );
        store.save().unwrap();
    }

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let profile = body_json(res).await;

    let mixin_yaml = profile["mixin_yaml"].as_str().unwrap();
    let mixin_root: YamlValue = serde_yaml::from_str(mixin_yaml).unwrap();
    let mixin_map = mixin_root.as_mapping().expect("mixin must be a mapping");
    assert!(
        !mixin_map.contains_key(YamlValue::String("proxies".to_string())),
        "legacy stored full config should be normalized on GET"
    );
    assert!(
        !mixin_map.contains_key(YamlValue::String("proxy-providers".to_string())),
        "legacy stored full config should expose split provider data on GET"
    );

    let extra_proxies_root: YamlValue =
        serde_yaml::from_str(profile["extra_proxies_yaml"].as_str().unwrap()).unwrap();
    assert!(
        extra_proxies_root
            .as_sequence()
            .expect("extra_proxies_yaml must be a sequence")
            .iter()
            .any(|proxy| {
                proxy
                    .get("name")
                    .and_then(YamlValue::as_str)
                    .is_some_and(|name| name == "custom-direct")
            }),
        "legacy stored proxies should be extracted on GET"
    );

    let extra_providers_root: YamlValue =
        serde_yaml::from_str(profile["extra_proxy_providers_yaml"].as_str().unwrap()).unwrap();
    assert!(
        extra_providers_root
            .as_mapping()
            .expect("extra_proxy_providers_yaml must be a mapping")
            .contains_key(YamlValue::String("providerA".to_string())),
        "legacy stored provider map should be extracted on GET"
    );

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=mihomo")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let sub_text = body_text(res).await;
    let yaml: YamlValue = serde_yaml::from_str(&sub_text).unwrap();

    assert!(
        yaml.get("proxy-providers")
            .and_then(YamlValue::as_mapping)
            .is_some_and(|providers| {
                providers.contains_key(YamlValue::String("providerA".to_string()))
            }),
        "legacy stored full config should also be normalized on render"
    );
    assert!(
        yaml.get("proxies")
            .and_then(YamlValue::as_sequence)
            .is_some_and(|proxies| {
                proxies.iter().any(|proxy| {
                    proxy
                        .get("name")
                        .and_then(YamlValue::as_str)
                        .is_some_and(|name| name == "custom-direct")
                })
            }),
        "legacy stored extra proxies should remain visible in rendered subscriptions"
    );

    let groups = yaml
        .get("proxy-groups")
        .and_then(YamlValue::as_sequence)
        .expect("proxy-groups must exist");
    assert!(
        groups
            .iter()
            .any(|g| g.get("name").and_then(YamlValue::as_str) == Some("🛣️ JP/HK/TW")),
        "rendered legacy profile should inject the combined outer group"
    );
    for expected in ["🛣️ Japan", "🔒 Japan"] {
        let group = groups
            .iter()
            .find(|g| g.get("name").and_then(YamlValue::as_str) == Some(expected))
            .expect("compat region group should survive render");
        assert_eq!(
            group.get("type").and_then(YamlValue::as_str),
            Some("select"),
            "compat region group should be passive: {expected}"
        );
    }
}

#[tokio::test]
async fn admin_user_mihomo_profile_get_returns_raw_conflicting_legacy_provider_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id.clone();

    {
        let mut store = store.lock().await;
        store.state_mut().user_mihomo_profiles.insert(
            user_id.clone(),
            crate::state::UserMihomoProfile {
                mixin_yaml: r#"port: 0
proxy-providers:
  providerA:
    type: http
    path: ./provider-a-from-mixin.yaml
    url: https://example.com/sub-a-from-mixin
rules: []
"#
                .to_string(),
                extra_proxies_yaml: "".to_string(),
                extra_proxy_providers_yaml: r#"providerA:
  type: http
  path: ./provider-a-from-extra.yaml
  url: https://example.com/sub-a-from-extra
"#
                .to_string(),
            },
        );
        store.save().unwrap();
    }

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let profile = body_json(res).await;
    assert_eq!(
        profile["mixin_yaml"],
        r#"port: 0
proxy-providers:
  providerA:
    type: http
    path: ./provider-a-from-mixin.yaml
    url: https://example.com/sub-a-from-mixin
rules: []
"#
    );
    assert_eq!(
        profile["extra_proxy_providers_yaml"],
        r#"providerA:
  type: http
  path: ./provider-a-from-extra.yaml
  url: https://example.com/sub-a-from-extra
"#
    );
}

#[tokio::test]
async fn subscription_format_mihomo_renders_conflicting_legacy_provider_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id.clone();
    let token = fixtures.subscription_token;

    {
        let mut store = store.lock().await;
        store.state_mut().user_mihomo_profiles.insert(
            user_id,
            crate::state::UserMihomoProfile {
                mixin_yaml: r#"port: 0
proxy-providers:
  providerA:
    type: http
    path: ./provider-a-from-mixin.yaml
    url: https://example.com/sub-a-from-mixin
proxy-groups:
  - name: Auto
    type: select
    use: [providerA]
rules: []
"#
                .to_string(),
                extra_proxies_yaml: "".to_string(),
                extra_proxy_providers_yaml: r#"providerA:
  type: http
  path: ./provider-a-from-extra.yaml
  url: https://example.com/sub-a-from-extra
"#
                .to_string(),
            },
        );
        store.save().unwrap();
    }

    let res = app
        .clone()
        .oneshot(req("GET", &format!("/api/sub/{token}?format=mihomo")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let yaml: YamlValue = serde_yaml::from_str(&body_text(res).await).unwrap();
    let provider_a = yaml
        .get("proxy-providers")
        .and_then(YamlValue::as_mapping)
        .and_then(|map| map.get(YamlValue::String("providerA".to_string())))
        .and_then(YamlValue::as_mapping)
        .expect("providerA should still render from extra providers");
    assert_eq!(
        provider_a
            .get(YamlValue::String("path".to_string()))
            .and_then(YamlValue::as_str),
        Some("./provider-a-from-extra.yaml")
    );
}

#[tokio::test]
async fn admin_user_mihomo_profile_get_returns_raw_invalid_stored_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id.clone();

    {
        let mut store = store.lock().await;
        store.state_mut().user_mihomo_profiles.insert(
            user_id.clone(),
            crate::state::UserMihomoProfile {
                mixin_yaml: "port: [
"
                .to_string(),
                extra_proxies_yaml: "".to_string(),
                extra_proxy_providers_yaml: "".to_string(),
            },
        );
        store.save().unwrap();
    }

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let profile = body_json(res).await;
    assert_eq!(
        profile["mixin_yaml"],
        "port: [
"
    );
    assert_eq!(profile["extra_proxies_yaml"], "");
    assert_eq!(profile["extra_proxy_providers_yaml"], "");
}

#[tokio::test]
async fn admin_user_mihomo_profile_put_rejects_legacy_template_yaml_field() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id;

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
            json!({
              "template_yaml": "port: 0
rules: []
",
              "extra_proxies_yaml": "",
              "extra_proxy_providers_yaml": "",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn subscription_format_mihomo_renders_without_proxy_providers() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id;
    let token = fixtures.subscription_token;

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
            json!({
              "mixin_yaml": r#"port: 0
proxy-groups:
  - name: "Auto"
    type: select
    proxies: ["🛣️ JP/HK/TW"]
rules: []
"#,
              "extra_proxies_yaml": r#"- name: "custom-direct"
  type: ss
  server: custom.example.com
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: "abc:def"
  udp: true
"#,
              "extra_proxy_providers_yaml": "",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=mihomo")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let sub_text = body_text(res).await;
    let yaml: YamlValue = serde_yaml::from_str(&sub_text).unwrap();

    let providers = yaml
        .get("proxy-providers")
        .and_then(YamlValue::as_mapping)
        .expect("proxy-providers must exist");
    assert!(
        providers.is_empty(),
        "expected empty proxy-providers mapping"
    );

    let groups = yaml
        .get("proxy-groups")
        .and_then(YamlValue::as_sequence)
        .expect("proxy-groups must exist");
    let outer_group = groups
        .iter()
        .find(|g| g.get("name").and_then(YamlValue::as_str) == Some("🛣️ JP/HK/TW"))
        .expect("expected built-in outer group 🛣️ JP/HK/TW");
    assert_eq!(
        outer_group
            .get("use")
            .and_then(YamlValue::as_sequence)
            .map(|items| items.len()),
        Some(0),
        "expected 🛣️ JP/HK/TW to tolerate an empty provider pool"
    );
    assert_eq!(
        outer_group.get("proxies").and_then(YamlValue::as_sequence),
        Some(&vec![YamlValue::String("DIRECT".to_string())])
    );
    for expected in [
        "🛣️ Japan",
        "🌟 Japan",
        "🔒 Japan",
        "🤯 Japan",
        "🛣️ HongKong",
        "🌟 HongKong",
        "🔒 HongKong",
        "🤯 HongKong",
        "🛣️ Taiwan",
        "🌟 Taiwan",
        "🔒 Taiwan",
        "🤯 Taiwan",
        "🛣️ Korea",
        "🌟 Korea",
        "🔒 Korea",
        "🤯 Korea",
    ] {
        assert!(
            groups
                .iter()
                .any(|g| g.get("name").and_then(YamlValue::as_str) == Some(expected)),
            "compat region group missing from output: {expected}"
        );
    }

    let proxies = yaml
        .get("proxies")
        .and_then(YamlValue::as_sequence)
        .expect("proxies must exist");
    assert!(
        proxies.iter().any(|proxy| {
            proxy
                .get("name")
                .and_then(YamlValue::as_str)
                .is_some_and(|name| name == "custom-direct")
        }),
        "expected extra_proxies_yaml to remain visible without proxy-providers"
    );
}

#[tokio::test]
async fn admin_user_mihomo_profile_put_rejects_invalid_yaml_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let user_id = fixtures.user_id;
    let cases = vec![
        (
            json!({
              "mixin_yaml": "- not-a-mapping
            ",
              "extra_proxies_yaml": "",
              "extra_proxy_providers_yaml": "",
            }),
            "mixin_yaml must be a yaml mapping",
        ),
        (
            json!({
              "mixin_yaml": "port: 0
            ",
              "extra_proxies_yaml": "k: v
            ",
              "extra_proxy_providers_yaml": "",
            }),
            "extra_proxies_yaml must be a yaml sequence or empty string",
        ),
        (
            json!({
              "mixin_yaml": "port: 0
            ",
              "extra_proxies_yaml": "",
              "extra_proxy_providers_yaml": "- not-a-mapping
            ",
            }),
            "extra_proxy_providers_yaml must be a yaml mapping or empty string",
        ),
        (
            json!({
              "mixin_yaml": "port: 0
proxies:
  - name: x
    type: ss
    server: example.com
    port: 443
    cipher: 2022-blake3-aes-128-gcm
    password: abc:def
    udp: true
",
              "extra_proxies_yaml": "- name: y
  type: ss
  server: example.org
  port: 443
  cipher: 2022-blake3-aes-128-gcm
  password: ghi:jkl
  udp: true
",
              "extra_proxy_providers_yaml": "",
            }),
            "mixin_yaml.proxies cannot be combined with extra_proxies_yaml",
        ),
        (
            json!({
              "mixin_yaml": "port: 0
proxy-providers:
  providerA:
    type: http
    path: ./provider-a.yaml
    url: https://example.com/sub-a
",
              "extra_proxies_yaml": "",
              "extra_proxy_providers_yaml": "providerB:
  type: http
  path: ./provider-b.yaml
  url: https://example.com/sub-b
",
            }),
            "mixin_yaml.proxy-providers cannot be combined with extra_proxy_providers_yaml",
        ),
    ];

    for (payload, expected_message) in cases {
        let res = app
            .clone()
            .oneshot(req_authed_json(
                "PUT",
                &format!("/api/admin/users/{user_id}/subscription-mihomo-profile"),
                payload,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = body_json(res).await;
        assert_eq!(body["error"]["code"], "invalid_request");
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains(expected_message)),
            "expected message to contain: {expected_message}"
        );
    }
}

#[tokio::test]
async fn subscription_token_reset_invalidates_old_token() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let old_token = fixtures.subscription_token;
    let user_id = fixtures.user_id;

    let res = app
        .clone()
        .oneshot(req_authed(
            "POST",
            &format!("/api/admin/users/{user_id}/reset-token"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let new_token = json["subscription_token"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req("GET", &format!("/api/sub/{old_token}?format=raw")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{new_token}?format=raw")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn subscription_removed_access_not_in_output() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let token = fixtures.subscription_token;
    let user_id = fixtures.user_id;
    let password = fixtures.ss2022_password;

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({
              "items": []
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=raw")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let raw_body = body_text(res).await;
    assert!(!raw_body.contains(&password));
}

#[tokio::test]
async fn put_user_access_empty_removes_usage_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let membership_key = fixtures.membership_key.clone();
    let user_id = fixtures.user_id.clone();
    let banned_at = "2025-12-18T00:00:00Z".to_string();

    {
        let mut store = store.lock().await;
        store.set_quota_banned(&membership_key, banned_at).unwrap();
        assert!(
            store
                .get_membership_usage(&membership_key)
                .unwrap()
                .quota_banned
        );
    }
    record_inbound_ip_usage_samples(
        &store,
        crate::inbound_ip_usage::floor_minute(chrono::Utc::now()),
        false,
        vec![crate::inbound_ip_usage::InboundIpMinuteSample {
            membership_key: membership_key.clone(),
            user_id: user_id.clone(),
            node_id: fixtures.node_id.clone(),
            endpoint_id: fixtures.endpoint_id.clone(),
            endpoint_tag: fixtures.endpoint_tag.clone(),
            ips: vec!["203.0.113.7".to_string()],
        }],
    )
    .await;

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({ "items": [] }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let store = store.lock().await;
    assert!(store.get_membership_usage(&membership_key).is_none());
    assert!(
        !store
            .inbound_ip_usage()
            .memberships
            .contains_key(&membership_key)
    );
}

#[tokio::test]
async fn admin_alerts_local_reports_quota_banned_membership() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let user = body_json(res).await;
    let user_id = user["user_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let endpoint = body_json(res).await;
    let endpoint_id = endpoint["endpoint_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/access"),
            json!({
              "items": [{
                "endpoint_id": endpoint_id
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    drop(res);
    let membership_key = membership_key(&user_id, &endpoint_id);

    let banned_at = "2025-12-18T00:00:00Z".to_string();
    {
        let mut store = store.lock().await;
        store
            .set_quota_banned(&membership_key, banned_at.clone())
            .unwrap();
    }

    let res = app
        .oneshot(req_authed("GET", "/api/admin/alerts?scope=local"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["partial"], false);
    assert_eq!(json["unreachable_nodes"], json!([]));

    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item["type"], "quota_banned_membership");
    assert_eq!(item["membership_key"], membership_key);
    assert_eq!(item["user_id"], user_id);
    assert_eq!(item["endpoint_id"], endpoint_id);
    assert_eq!(item["owner_node_id"], node_id);
    assert_eq!(item["quota_banned"], true);
    assert_eq!(item["quota_banned_at"], banned_at);
    assert_eq!(
        item["message"],
        "quota enforced on owner node (membership is blocked)"
    );
    assert_eq!(
        item["action_hint"],
        "wait for rollover/unban or adjust quota policy"
    );
}

#[tokio::test]
async fn admin_alerts_reports_partial_when_node_unreachable() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let remote_node_id = new_ulid_string();
    {
        let mut store = store.lock().await;
        store
            .upsert_node(Node {
                node_id: remote_node_id.clone(),
                node_name: "node-unreachable".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:1".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            })
            .unwrap();
    }

    let res = app
        .oneshot(req_authed("GET", "/api/admin/alerts"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    assert_eq!(json["partial"], true);
    assert_eq!(json["unreachable_nodes"], json!([remote_node_id]));
}

#[test]
fn config_ip_usage_geo_db_missing_when_mmdb_files_are_unreadable() {
    let tmp = tempfile::tempdir().unwrap();
    let city_path = tmp.path().join("GeoLite2-City.mmdb");
    let asn_path = tmp.path().join("GeoLite2-ASN.mmdb");
    std::fs::write(&city_path, b"not-a-mmdb").unwrap();
    std::fs::write(&asn_path, b"not-a-mmdb").unwrap();

    let mut config = test_config(tmp.path().to_path_buf());
    config.ip_usage_city_db_path = city_path.display().to_string();
    config.ip_usage_asn_db_path = asn_path.display().to_string();

    let resolver = crate::ip_geo_db::SharedGeoResolver::new(&config);
    assert!(crate::inbound_ip_usage::GeoLookup::is_missing(&resolver));
}

#[tokio::test]
async fn node_ip_usage_returns_series_timeline_and_ip_list() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let (
        node_id,
        membership_one,
        membership_two,
        endpoint_one_tag,
        endpoint_two_tag,
        minute0,
        minute1,
    ) = {
        let mut store = store.lock().await;
        let node_id = store
            .state()
            .nodes
            .keys()
            .next()
            .cloned()
            .expect("bootstrap node");
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint_one = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        let endpoint_two = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8389,
                json!({}),
            )
            .unwrap();
        crate::state::DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![
                endpoint_one.endpoint_id.clone(),
                endpoint_two.endpoint_id.clone(),
            ],
        }
        .apply(store.state_mut())
        .unwrap();

        let minute0 = crate::inbound_ip_usage::floor_minute(chrono::Utc::now())
            - chrono::Duration::minutes(1);
        let minute1 = minute0 + chrono::Duration::minutes(1);
        let resolver = crate::inbound_ip_usage::GeoResolver::new(None, None);
        let geo_db = resolver.geo_db();
        let membership_one = membership_key(&user.user_id, &endpoint_one.endpoint_id);
        let membership_two = membership_key(&user.user_id, &endpoint_two.endpoint_id);
        store
            .record_inbound_ip_usage_samples(
                minute0,
                geo_db.clone(),
                false,
                &[crate::inbound_ip_usage::InboundIpMinuteSample {
                    membership_key: membership_one.clone(),
                    user_id: user.user_id.clone(),
                    node_id: node_id.clone(),
                    endpoint_id: endpoint_one.endpoint_id.clone(),
                    endpoint_tag: endpoint_one.tag.clone(),
                    ips: vec!["203.0.113.7".to_string()],
                }],
                &resolver,
            )
            .unwrap();
        store
            .record_inbound_ip_usage_samples(
                minute1,
                geo_db,
                false,
                &[
                    crate::inbound_ip_usage::InboundIpMinuteSample {
                        membership_key: membership_one.clone(),
                        user_id: user.user_id.clone(),
                        node_id: node_id.clone(),
                        endpoint_id: endpoint_one.endpoint_id.clone(),
                        endpoint_tag: endpoint_one.tag.clone(),
                        ips: vec!["203.0.113.7".to_string()],
                    },
                    crate::inbound_ip_usage::InboundIpMinuteSample {
                        membership_key: membership_two.clone(),
                        user_id: user.user_id.clone(),
                        node_id: node_id.clone(),
                        endpoint_id: endpoint_two.endpoint_id.clone(),
                        endpoint_tag: endpoint_two.tag.clone(),
                        ips: vec!["203.0.113.7".to_string(), "198.51.100.9".to_string()],
                    },
                ],
                &resolver,
            )
            .unwrap();

        (
            node_id,
            membership_one,
            membership_two,
            endpoint_one.tag,
            endpoint_two.tag,
            minute0.to_rfc3339(),
            minute1.to_rfc3339(),
        )
    };

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/nodes/{node_id}/ip-usage?window=24h"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;

    assert_eq!(json["node"]["node_id"], node_id);
    assert_eq!(json["window"], "24h");
    assert_eq!(json["window_end"], minute1);
    assert_eq!(json["unique_ip_series"].as_array().unwrap().len(), 24 * 60);
    assert_eq!(series_count_at(&json["unique_ip_series"], &minute0), 1);
    assert_eq!(series_count_at(&json["unique_ip_series"], &minute1), 2);
    assert_eq!(
        warning_codes(&json["warnings"]),
        vec!["geo_db_missing".to_string()]
    );

    let timeline = json["timeline"].as_array().unwrap();
    assert_eq!(timeline.len(), 3);
    let merged_lane = timeline
        .iter()
        .find(|item| item["endpoint_tag"] == endpoint_one_tag && item["ip"] == "203.0.113.7")
        .expect("merged lane");
    assert_eq!(merged_lane["minutes"], 2);
    assert_eq!(merged_lane["segments"].as_array().unwrap().len(), 1);
    assert_eq!(merged_lane["segments"][0]["start_minute"], minute0);
    assert_eq!(merged_lane["segments"][0]["end_minute"], minute1);

    let ip_entry = json["ips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["ip"] == "203.0.113.7")
        .expect("ip entry");
    assert_eq!(ip_entry["minutes"], 2);
    let mut endpoint_tags = ip_entry["endpoint_tags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    endpoint_tags.sort();
    let mut expected_tags = vec![endpoint_one_tag, endpoint_two_tag];
    expected_tags.sort();
    assert_eq!(endpoint_tags, expected_tags);
    assert_eq!(ip_entry["region"], "");
    assert_eq!(ip_entry["operator"], "");

    let store = store.lock().await;
    assert!(store.get_membership_usage(&membership_one).is_none());
    assert!(store.get_membership_usage(&membership_two).is_none());
}

#[tokio::test]
async fn user_ip_usage_groups_local_data_and_merges_warnings() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let (user_id, node_id, endpoint_tag, minute) = {
        let mut store = store.lock().await;
        let node_id = store
            .state()
            .nodes
            .keys()
            .next()
            .cloned()
            .expect("bootstrap node");
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        crate::state::DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        let membership = membership_key(&user.user_id, &endpoint.endpoint_id);
        let minute = crate::inbound_ip_usage::floor_minute(chrono::Utc::now());
        let resolver = crate::inbound_ip_usage::GeoResolver::new(None, None);
        store
            .record_inbound_ip_usage_samples(
                minute,
                resolver.geo_db(),
                false,
                &[crate::inbound_ip_usage::InboundIpMinuteSample {
                    membership_key: membership,
                    user_id: user.user_id.clone(),
                    node_id: node_id.clone(),
                    endpoint_id: endpoint.endpoint_id,
                    endpoint_tag: endpoint.tag.clone(),
                    ips: vec!["203.0.113.9".to_string()],
                }],
                &resolver,
            )
            .unwrap();
        store
            .update_inbound_ip_usage(|usage| {
                usage.online_stats_unavailable = true;
            })
            .unwrap();

        (user.user_id, node_id, endpoint.tag, minute.to_rfc3339())
    };

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/ip-usage?window=24h"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;

    assert_eq!(json["user"]["user_id"], user_id);
    assert_eq!(json["window"], "24h");
    assert_eq!(json["partial"], false);
    assert_eq!(json["unreachable_nodes"], json!([]));
    assert_eq!(
        warning_codes(&json["warnings"]),
        vec![
            "geo_db_missing".to_string(),
            "online_stats_unavailable".to_string()
        ]
    );

    let groups = json["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    let group = &groups[0];
    assert_eq!(group["node"]["node_id"], node_id);
    assert_eq!(group["window_end"], minute);
    assert_eq!(
        warning_codes(&group["warnings"]),
        vec![
            "geo_db_missing".to_string(),
            "online_stats_unavailable".to_string()
        ]
    );
    assert_eq!(series_count_at(&group["unique_ip_series"], &minute), 1);
    assert_eq!(
        group["timeline"].as_array().unwrap()[0]["endpoint_tag"],
        endpoint_tag
    );
    assert_eq!(group["ips"].as_array().unwrap()[0]["ip"], "203.0.113.9");
}

#[tokio::test]
async fn user_ip_usage_marks_remote_nodes_as_partial() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let (user_id, local_node_id, remote_node_id) = {
        let mut store = store.lock().await;
        let local_node_id = store
            .state()
            .nodes
            .keys()
            .next()
            .cloned()
            .expect("bootstrap node");
        let remote_node_id = new_ulid_string();
        store
            .upsert_node(Node {
                node_id: remote_node_id.clone(),
                node_name: "node-remote".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:1".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            })
            .unwrap();

        let user = store.create_user("alice".to_string(), None).unwrap();
        let local_endpoint = store
            .create_endpoint(
                local_node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        let remote_endpoint = store
            .create_endpoint(
                remote_node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                9393,
                json!({}),
            )
            .unwrap();
        crate::state::DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![
                local_endpoint.endpoint_id.clone(),
                remote_endpoint.endpoint_id.clone(),
            ],
        }
        .apply(store.state_mut())
        .unwrap();

        let minute = crate::inbound_ip_usage::floor_minute(chrono::Utc::now());
        let resolver = crate::inbound_ip_usage::GeoResolver::new(None, None);
        store
            .record_inbound_ip_usage_samples(
                minute,
                resolver.geo_db(),
                false,
                &[crate::inbound_ip_usage::InboundIpMinuteSample {
                    membership_key: membership_key(&user.user_id, &local_endpoint.endpoint_id),
                    user_id: user.user_id.clone(),
                    node_id: local_node_id.clone(),
                    endpoint_id: local_endpoint.endpoint_id,
                    endpoint_tag: local_endpoint.tag,
                    ips: vec!["203.0.113.20".to_string()],
                }],
                &resolver,
            )
            .unwrap();

        (user.user_id, local_node_id, remote_node_id)
    };

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/ip-usage?window=24h"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;

    assert_eq!(json["partial"], true);
    assert_eq!(json["unreachable_nodes"], json!([remote_node_id]));
    let groups = json["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["node"]["node_id"], local_node_id);
}

#[tokio::test]
async fn ip_usage_rejects_invalid_window_values() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let (node_id, user_id) = {
        let mut store = store.lock().await;
        let node_id = store
            .state()
            .nodes
            .keys()
            .next()
            .cloned()
            .expect("bootstrap node");
        let user = store.create_user("alice".to_string(), None).unwrap();
        (node_id, user.user_id)
    };

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/nodes/{node_id}/ip-usage?window=nope"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");

    let res = app
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/ip-usage?window=nope"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn admin_delete_user_removes_usage_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let fixtures = setup_subscription_fixtures(&tmp, &app).await;
    let membership_key = fixtures.membership_key.clone();
    let user_id = fixtures.user_id.clone();

    {
        let mut store = store.lock().await;
        store
            .set_quota_banned(&membership_key, "2025-12-18T00:00:00Z".to_string())
            .unwrap();
        assert!(store.get_membership_usage(&membership_key).is_some());
    }
    record_inbound_ip_usage_samples(
        &store,
        crate::inbound_ip_usage::floor_minute(chrono::Utc::now()),
        false,
        vec![crate::inbound_ip_usage::InboundIpMinuteSample {
            membership_key: membership_key.clone(),
            user_id: user_id.clone(),
            node_id: fixtures.node_id.clone(),
            endpoint_id: fixtures.endpoint_id.clone(),
            endpoint_tag: fixtures.endpoint_tag.clone(),
            ips: vec!["203.0.113.7".to_string()],
        }],
    )
    .await;

    let res = app
        .oneshot(req_authed("DELETE", &format!("/api/admin/users/{user_id}")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let store = store.lock().await;
    assert!(store.get_membership_usage(&membership_key).is_none());
    assert!(
        !store
            .inbound_ip_usage()
            .memberships
            .contains_key(&membership_key)
    );
}

#[tokio::test]
async fn subscription_invalid_format_returns_400_invalid_request() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let token = setup_subscription_fixtures(&tmp, &app)
        .await
        .subscription_token;

    let res = app
        .oneshot(req("GET", &format!("/api/sub/{token}?format=wat")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn subscription_unknown_token_returns_404_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app.oneshot(req("GET", "/api/sub/sub_nope")).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_found");
}

#[tokio::test]
async fn persistence_smoke_user_roundtrip_via_api() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();

    let config = test_config(data_dir.clone());
    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(tmp.path()).unwrap();

    let store =
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
            .unwrap();
    let store = Arc::new(Mutex::new(store));
    let raft = leader_raft(store.clone(), &cluster);
    let xray_health = XrayHealthHandle::new_unknown();
    let cloudflared_health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
    let (node_runtime, _node_runtime_task) = crate::node_runtime::spawn_node_runtime_monitor(
        Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let geo_db_update = test_geo_db_update_handle(&config, store.clone());
    let app = build_router(
        config.clone(),
        store,
        crate::reconcile::ReconcileHandle::noop(),
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster.clone(),
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
        geo_db_update,
    );

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice",
              "cycle_policy_default": "by_user",
              "cycle_day_of_month_default": 1
            }),
        ))
        .await
        .unwrap();
    let created = body_json(res).await;
    let user_id = created["user_id"].as_str().unwrap().to_string();

    drop(app);

    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(tmp.path()).unwrap();
    let store =
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
            .unwrap();
    let store = Arc::new(Mutex::new(store));
    let raft = leader_raft(store.clone(), &cluster);
    let xray_health = XrayHealthHandle::new_unknown();
    let cloudflared_health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
    let (node_runtime, _node_runtime_task) = crate::node_runtime::spawn_node_runtime_monitor(
        Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let geo_db_update = test_geo_db_update_handle(&config, store.clone());
    let app = build_router(
        config,
        store,
        crate::reconcile::ReconcileHandle::noop(),
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
        geo_db_update,
    );

    let res = app
        .oneshot(req_authed("GET", "/api/admin/users"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    let items = listed["items"].as_array().unwrap();
    assert!(items.iter().any(|u| u["user_id"] == user_id));
}

#[tokio::test]
async fn vless_endpoint_creation_persists_reality_materials_and_derived_uuid_is_uuidv4() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "vless_reality_vision_tcp",
              "port": 443,
              "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let endpoint = body_json(res).await;
    let endpoint_id = endpoint["endpoint_id"].as_str().unwrap();

    let meta = &endpoint["meta"];
    let short_ids = meta["short_ids"].as_array().unwrap();
    assert_eq!(short_ids.len(), 1);
    let short_id = short_ids[0].as_str().unwrap();
    assert_eq!(short_id.len(), 16);
    assert!(short_id.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(meta["active_short_id"], short_ids[0]);

    let priv_key = meta["reality_keys"]["private_key"].as_str().unwrap();
    let pub_key = meta["reality_keys"]["public_key"].as_str().unwrap();
    let priv_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(priv_key)
        .unwrap();
    let pub_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(pub_key)
        .unwrap();
    assert_eq!(priv_bytes.len(), 32);
    assert_eq!(pub_bytes.len(), 32);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    let user = body_json(res).await;
    let user_id = user["user_id"].as_str().unwrap();
    let credential_epoch = user
        .get("credential_epoch")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .expect("cluster ca key pem");

    let uuid =
        crate::credentials::derive_vless_uuid(&cluster_ca_key_pem, user_id, credential_epoch)
            .expect("derive vless uuid");
    assert!(Uuid::parse_str(&uuid).is_ok());
    assert_eq!(uuid.chars().nth(14).unwrap(), '4');
    assert!(!is_ulid_string(&uuid));

    let email = crate::state::membership_xray_email(user_id, endpoint_id);
    assert_eq!(email, format!("m:{user_id}::{endpoint_id}"));
}

#[tokio::test]
async fn ss2022_endpoint_creation_persists_server_psk_and_password_uses_server_and_user_psk() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let endpoint = body_json(res).await;
    let _endpoint_id = endpoint["endpoint_id"].as_str().unwrap();

    assert_eq!(endpoint["meta"]["method"], "2022-blake3-aes-128-gcm");
    let server_psk_b64 = endpoint["meta"]["server_psk_b64"].as_str().unwrap();
    let server_psk = base64::engine::general_purpose::STANDARD
        .decode(server_psk_b64)
        .unwrap();
    assert_eq!(server_psk.len(), 16);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({
              "display_name": "alice"
            }),
        ))
        .await
        .unwrap();
    let user = body_json(res).await;
    let user_id = user["user_id"].as_str().unwrap();
    let credential_epoch = user
        .get("credential_epoch")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .expect("cluster ca key pem");

    let user_psk_b64 = crate::credentials::derive_ss2022_user_psk_b64(
        &cluster_ca_key_pem,
        user_id,
        credential_epoch,
    )
    .expect("derive ss2022 user_psk");
    let password = ss2022_password(server_psk_b64, &user_psk_b64);
    let (server_part, user_part) = password.split_once(':').unwrap();
    assert_eq!(server_part, server_psk_b64);
    assert_eq!(user_part, user_psk_b64);
    let user_psk = base64::engine::general_purpose::STANDARD
        .decode(user_part)
        .unwrap();
    assert_eq!(user_psk.len(), 16);
}

#[tokio::test]
async fn rotate_shortid_updates_persisted_meta_and_rejects_non_vless_endpoints() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "vless_reality_vision_tcp",
              "port": 443,
              "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
              }
            }),
        ))
        .await
        .unwrap();
    let endpoint = body_json(res).await;
    let endpoint_id = endpoint["endpoint_id"].as_str().unwrap().to_string();
    let before_active = endpoint["meta"]["active_short_id"]
        .as_str()
        .unwrap()
        .to_string();

    let res = app
        .clone()
        .oneshot(req_authed(
            "POST",
            &format!("/api/admin/endpoints/{endpoint_id}/rotate-shortid"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let rotated = body_json(res).await;
    assert_eq!(rotated["endpoint_id"], endpoint_id);
    assert_ne!(rotated["active_short_id"], before_active);
    assert_eq!(rotated["short_ids"].as_array().unwrap().len(), 2);

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/endpoints/{endpoint_id}"),
        ))
        .await
        .unwrap();
    let persisted = body_json(res).await;
    assert_eq!(
        persisted["meta"]["active_short_id"],
        rotated["active_short_id"]
    );
    assert_eq!(persisted["meta"]["short_ids"], rotated["short_ids"]);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    let ss_endpoint = body_json(res).await;
    let ss_endpoint_id = ss_endpoint["endpoint_id"].as_str().unwrap();

    let res = app
        .oneshot(req_authed(
            "POST",
            &format!("/api/admin/endpoints/{ss_endpoint_id}/rotate-shortid"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn user_quota_summaries_include_membership_usage() {
    let tmp = TempDir::new().unwrap();
    let (_app, store) = app_with(&tmp, ReconcileHandle::noop());

    let local_node_id = {
        let store = store.lock().await;
        store
            .state()
            .nodes
            .keys()
            .next()
            .cloned()
            .expect("bootstrap node_id")
    };

    let user_id = {
        let mut store = store.lock().await;
        let user = store.create_user("User".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                local_node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        crate::state::DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        let key = membership_key(&user.user_id, &endpoint.endpoint_id);
        store
            .apply_membership_usage_sample(
                &key,
                "cycle-start".to_string(),
                "cycle-end".to_string(),
                600,
                100,
                "seen".to_string(),
            )
            .unwrap();
        store.save().unwrap();
        user.user_id
    };

    let items = {
        let store = store.lock().await;
        super::build_local_user_quota_summaries(&store, &local_node_id).unwrap()
    };
    let user = items
        .iter()
        .find(|i| i.user_id == user_id)
        .expect("missing user summary");
    assert_eq!(
        user.quota_limit_kind,
        super::AdminUserQuotaLimitKind::Unlimited
    );
    assert_eq!(user.quota_limit_bytes, 0);
    assert_eq!(user.used_bytes, 700);
    assert_eq!(user.remaining_bytes, 0);
}

#[tokio::test]
async fn user_node_quota_status_includes_membership_usage() {
    let tmp = TempDir::new().unwrap();
    let (_app, store) = app_with(&tmp, ReconcileHandle::noop());

    let local_node_id = {
        let store = store.lock().await;
        store
            .state()
            .nodes
            .keys()
            .next()
            .cloned()
            .expect("bootstrap node_id")
    };

    let user_id = {
        let mut store = store.lock().await;
        // Force a deterministic UTC cycle window for stable tests.
        let node = store.get_node(&local_node_id).unwrap();
        let _ = store
            .upsert_node(Node {
                quota_reset: NodeQuotaReset::Monthly {
                    day_of_month: 1,
                    tz_offset_minutes: Some(0),
                },
                ..node
            })
            .unwrap();

        let user = store.create_user("User".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                local_node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        crate::state::DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        let (cycle_start, cycle_end) = crate::cycle::current_cycle_window_at(
            crate::cycle::CycleTimeZone::FixedOffsetMinutes {
                tz_offset_minutes: 0,
            },
            1,
            chrono::Utc::now(),
        )
        .unwrap();
        let key = membership_key(&user.user_id, &endpoint.endpoint_id);
        store
            .apply_membership_usage_sample(
                &key,
                cycle_start.to_rfc3339(),
                cycle_end.to_rfc3339(),
                600,
                100,
                "seen".to_string(),
            )
            .unwrap();
        store.save().unwrap();
        user.user_id
    };

    let items = {
        let store = store.lock().await;
        super::build_local_user_node_quota_status(&store, &local_node_id, &user_id).unwrap()
    };
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.user_id, user_id);
    assert_eq!(item.node_id, local_node_id);
    assert_eq!(item.used_bytes, 700);
    assert_eq!(item.remaining_bytes, 0);
    assert_eq!(item.quota_reset_source, QuotaResetSource::Node);
    assert!(item.cycle_end_at.is_some());
}

#[tokio::test]
async fn user_node_quota_status_includes_usage_when_node_reset_is_unlimited() {
    let tmp = TempDir::new().unwrap();
    let (_app, store) = app_with(&tmp, ReconcileHandle::noop());

    let local_node_id = {
        let store = store.lock().await;
        store
            .state()
            .nodes
            .keys()
            .next()
            .cloned()
            .expect("bootstrap node_id")
    };

    let user_id = {
        let mut store = store.lock().await;
        let node = store.get_node(&local_node_id).unwrap();
        let _ = store
            .upsert_node(Node {
                quota_reset: NodeQuotaReset::Unlimited {
                    tz_offset_minutes: Some(0),
                },
                ..node
            })
            .unwrap();

        let user = store.create_user("User".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                local_node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        crate::state::DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();

        let key = membership_key(&user.user_id, &endpoint.endpoint_id);
        store
            .apply_membership_usage_sample(
                &key,
                "1970-01-01T00:00:00Z".to_string(),
                "9999-12-31T23:59:59Z".to_string(),
                600,
                100,
                "seen".to_string(),
            )
            .unwrap();
        store.save().unwrap();
        user.user_id
    };

    let items = {
        let store = store.lock().await;
        super::build_local_user_node_quota_status(&store, &local_node_id, &user_id).unwrap()
    };
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.user_id, user_id);
    assert_eq!(item.node_id, local_node_id);
    assert_eq!(item.used_bytes, 700);
    assert_eq!(item.remaining_bytes, 0);
    assert_eq!(item.quota_reset_source, QuotaResetSource::Node);
    assert!(item.cycle_end_at.is_none());
}

#[tokio::test]
async fn user_quota_summaries_ignore_memberships_for_missing_users() {
    let tmp = TempDir::new().unwrap();
    let (_app, store) = app_with(&tmp, ReconcileHandle::noop());

    let local_node_id = {
        let store = store.lock().await;
        store
            .state()
            .nodes
            .keys()
            .next()
            .cloned()
            .expect("bootstrap node_id")
    };

    {
        let mut store = store.lock().await;
        let endpoint = store
            .create_endpoint(
                local_node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        store.state_mut().node_user_endpoint_memberships.insert(
            crate::state::NodeUserEndpointMembership {
                user_id: "missing-user".to_string(),
                node_id: local_node_id.clone(),
                endpoint_id: endpoint.endpoint_id.clone(),
            },
        );
        let key = membership_key("missing-user", &endpoint.endpoint_id);
        store
            .apply_membership_usage_sample(
                &key,
                "cycle-start".to_string(),
                "cycle-end".to_string(),
                600,
                100,
                "seen".to_string(),
            )
            .unwrap();
        store.save().unwrap();
    }

    let items = {
        let store = store.lock().await;
        super::build_local_user_quota_summaries(&store, &local_node_id).unwrap()
    };
    assert!(items.iter().all(|i| i.user_id != "missing-user"));
}

#[tokio::test]
async fn user_node_quota_status_returns_404_for_missing_user() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .oneshot(req_authed(
            "GET",
            "/api/admin/users/missing-user/node-quotas/status?scope=local",
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_found");
}

#[tokio::test]
async fn endpoint_probe_run_status_shows_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("POST", "/api/admin/endpoints/probe/run"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let started = body_json(res).await;
    let run_id = started["run_id"].as_str().unwrap().to_string();

    // The probe runner might finish quickly when there are no endpoints. Poll a few times to
    // avoid flaky timing assumptions.
    for _ in 0..20 {
        let res = app
            .clone()
            .oneshot(req_authed(
                "GET",
                &format!("/api/admin/endpoints/probe/runs/{run_id}"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let status = body_json(res).await;

        assert_eq!(status["run_id"].as_str().unwrap(), run_id);
        let nodes = status["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(!nodes[0]["node_id"].as_str().unwrap().is_empty());

        let overall = status["status"].as_str().unwrap();
        if overall == "finished" || overall == "failed" {
            let progress = nodes[0]["progress"].as_object().unwrap();
            assert_eq!(progress["run_id"].as_str().unwrap(), run_id);
            assert!(!progress["hour"].as_str().unwrap().is_empty());
            assert!(!progress["config_hash"].as_str().unwrap().is_empty());
            assert!(!progress["updated_at"].as_str().unwrap().is_empty());
            return;
        }

        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    panic!("timeout waiting for endpoint probe run status to finish");
}

#[tokio::test]
async fn endpoint_probe_run_events_streams_sse() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("POST", "/api/admin/endpoints/probe/run"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let started = body_json(res).await;
    let run_id = started["run_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/endpoints/probe/runs/{run_id}/events"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let content_type = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.starts_with("text/event-stream"),
        "unexpected content-type: {content_type}"
    );

    let body = body_text(res).await;
    assert!(body.contains("event: hello"), "missing hello event: {body}");
    assert!(
        body.contains(&format!("\"run_id\":\"{run_id}\"")),
        "missing run_id in body: {body}"
    );
    assert!(
        body.contains("event: progress"),
        "missing progress event: {body}"
    );
}

#[tokio::test]
async fn reality_domains_list_returns_seeded_items() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/reality-domains"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    let items = body["items"].as_array().unwrap();
    assert!(items.len() >= 2);

    let server_names: Vec<&str> = items
        .iter()
        .filter_map(|v| v.get("server_name").and_then(|s| s.as_str()))
        .collect();
    assert!(server_names.contains(&"public.sn.files.1drv.com"));
    assert!(server_names.contains(&"public.bn.files.1drv.com"));
}

#[tokio::test]
async fn reality_domains_crud_and_reorder_works() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/reality-domains",
            json!({
              "server_name": "example.com",
              "disabled_node_ids": [node_id]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let domain_id = created["domain_id"].as_str().unwrap().to_string();
    assert!(is_ulid_string(&domain_id));

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/reality-domains"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    let items = listed["items"].as_array().unwrap();
    assert!(items.iter().any(|d| d["domain_id"] == domain_id));

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/reality-domains/{domain_id}"),
            json!({
              "disabled_node_ids": []
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let patched = body_json(res).await;
    assert_eq!(patched["domain_id"], domain_id);
    assert_eq!(patched["disabled_node_ids"].as_array().unwrap().len(), 0);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/reality-domains"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    let items = listed["items"].as_array().unwrap();
    let original_ids: Vec<String> = items
        .iter()
        .filter_map(|d| {
            d.get("domain_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    let mut desired_ids = original_ids.clone();
    desired_ids.reverse();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/reality-domains/reorder",
            json!({ "domain_ids": desired_ids }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/reality-domains"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    let reordered = listed["items"].as_array().unwrap();
    let got_ids: Vec<String> = reordered
        .iter()
        .filter_map(|d| {
            d.get("domain_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    assert_eq!(got_ids, original_ids.into_iter().rev().collect::<Vec<_>>());

    let res = app
        .clone()
        .oneshot(req_authed(
            "DELETE",
            &format!("/api/admin/reality-domains/{domain_id}"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/reality-domains"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    let items = listed["items"].as_array().unwrap();
    assert!(!items.iter().any(|d| d["domain_id"] == domain_id));
}

#[tokio::test]
async fn reality_domains_reject_invalid_server_names() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    for server_name in [
        "cc.c",
        "localhost",
        "https://example.com",
        "example.com:443",
    ] {
        let res = app
            .clone()
            .oneshot(req_authed_json(
                "POST",
                "/api/admin/reality-domains",
                json!({ "server_name": server_name }),
            ))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::BAD_REQUEST,
            "server_name should be rejected: {server_name}"
        );
        let body = body_json(res).await;
        assert_eq!(body["error"]["code"], "invalid_request");
    }
}
