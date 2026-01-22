use std::{net::SocketAddr, path::PathBuf, sync::Arc};

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

use crate::{
    cluster_metadata::ClusterMetadata,
    config::Config,
    domain::{EndpointKind, Node, NodeQuotaReset},
    http::build_router,
    id::{is_ulid_string, new_ulid_string},
    raft::{
        app::LocalRaft,
        types::{NodeMeta as RaftNodeMeta, raft_node_id_from_ulid},
    },
    reconcile::{ReconcileHandle, ReconcileRequest},
    state::{JsonSnapshotStore, StoreInit},
};

fn test_config(data_dir: PathBuf) -> Config {
    Config {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        xray_api_addr: SocketAddr::from(([127, 0, 0, 1], 10085)),
        data_dir,
        admin_token: "testtoken".to_string(),
        node_name: "node-1".to_string(),
        access_host: "".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        quota_poll_interval_secs: 10,
        quota_auto_unban: true,
    }
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

    let router = build_router(
        config,
        store.clone(),
        reconcile,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
    );
    (router, store)
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

struct SubscriptionFixtures {
    subscription_token: String,
    group_name: String,
    grant_id: String,
    user_id: String,
    endpoint_id: String,
    ss2022_password: String,
}

async fn setup_subscription_fixtures(
    app: &axum::Router,
    store: &Arc<Mutex<JsonSnapshotStore>>,
) -> SubscriptionFixtures {
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

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "test-group",
              "members": [{
                "user_id": user_id.clone(),
                "endpoint_id": endpoint_id.clone(),
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let group_detail = body_json(res).await;
    let password = group_detail["members"][0]["credentials"]["ss2022"]["password"]
        .as_str()
        .unwrap()
        .to_string();

    let grant_id = {
        let store = store.lock().await;
        store
            .list_grants()
            .into_iter()
            .find(|g| g.user_id == user_id && g.endpoint_id == endpoint_id)
            .map(|g| g.grant_id)
            .expect("expected grant to exist for subscription fixtures")
    };

    SubscriptionFixtures {
        subscription_token,
        group_name: "test-group".to_string(),
        grant_id,
        user_id,
        endpoint_id,
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
    assert!(json["error"]["message"].as_str().unwrap().len() > 0);
    assert!(json["error"]["details"].is_object());
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
    assert!(json["signed_cert_pem"].as_str().unwrap().len() > 0);
    assert!(json["cluster_ca_pem"].as_str().unwrap().len() > 0);

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

    let app = build_router(
        config,
        store,
        ReconcileHandle::noop(),
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
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
    assert!(json["user_id"].as_str().unwrap().len() > 0);
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
async fn set_user_node_quota_unifies_grants_and_can_be_listed() {
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

    // Create endpoint on that node.
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
    let created_ep = body_json(res).await;
    let endpoint_id = created_ep["endpoint_id"].as_str().unwrap().to_string();

    // Create grant group.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "test-group",
              "members": [{
                "user_id": user_id.clone(),
                "endpoint_id": endpoint_id.clone(),
                "enabled": true,
                "quota_limit_bytes": 123,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Set node quota (explicit source).
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/node-quotas/{node_id}"),
            json!({
              "quota_limit_bytes": 456,
              "quota_reset_source": "node"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let quota = body_json(res).await;
    assert_eq!(quota["user_id"], user_id);
    assert_eq!(quota["node_id"], node_id);
    assert_eq!(quota["quota_limit_bytes"], 456);
    assert_eq!(quota["quota_reset_source"], "node");

    // Update node quota without specifying source should preserve existing quota_reset_source.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{user_id}/node-quotas/{node_id}"),
            json!({
              "quota_limit_bytes": 789
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let quota = body_json(res).await;
    assert_eq!(quota["quota_limit_bytes"], 789);
    assert_eq!(quota["quota_reset_source"], "node");

    // List node quotas for user.
    let res = app
        .clone()
        .oneshot(req_authed(
            "GET",
            &format!("/api/admin/users/{user_id}/node-quotas"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["node_id"] == node_id && item["quota_limit_bytes"] == 789)
    );

    // Grant group member quota should be unified.
    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/grant-groups/test-group"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let fetched_group = body_json(res).await;
    assert_eq!(fetched_group["members"][0]["quota_limit_bytes"], 789);
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

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/nodes/{node_id}"),
            json!({
              "access_host": "node.example.com"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["node_id"], node_id);
    assert_eq!(updated["node_name"], original_node_name);
    assert_eq!(updated["access_host"], "node.example.com");
    assert_eq!(updated["api_base_url"], original_api_base_url);
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
              "access_host": "node.example.com"
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
    assert_eq!(json["admin_token_masked"], "*********");
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
async fn create_grant_group_with_missing_resources_returns_404_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let create = req_authed_json(
        "POST",
        "/api/admin/grant-groups",
        json!({
          "group_name": "test-group",
          "members": [{
            "user_id": new_ulid_string(),
            "endpoint_id": new_ulid_string(),
            "enabled": true,
            "quota_limit_bytes": 0,
            "note": null
          }]
        }),
    );
    let res = app.oneshot(create).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_found");
}

#[tokio::test]
async fn grant_group_replace_updates_member_note_and_allows_clear() {
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

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "test-group",
              "members": [{
                "user_id": user["user_id"],
                "endpoint_id": endpoint["endpoint_id"],
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            "/api/admin/grant-groups/test-group",
            json!({
              "members": [{
                "user_id": user["user_id"],
                "endpoint_id": endpoint["endpoint_id"],
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": "alice@node-1"
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/grant-groups/test-group"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["members"][0]["note"], "alice@node-1");

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            "/api/admin/grant-groups/test-group",
            json!({
              "members": [{
                "user_id": user["user_id"],
                "endpoint_id": endpoint["endpoint_id"],
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .oneshot(req_authed("GET", "/api/admin/grant-groups/test-group"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert!(updated["members"][0]["note"].is_null());
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
async fn post_admin_grant_groups_schedules_full_reconcile() {
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
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "test-group",
              "members": [{
                "user_id": user_id.clone(),
                "endpoint_id": endpoint_id,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
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
async fn put_admin_grant_group_schedules_full_reconcile() {
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

    // Create group first (and drain its reconcile request).
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "test-group",
              "members": [{
                "user_id": user_id,
                "endpoint_id": endpoint_id,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
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
            "/api/admin/grant-groups/test-group",
            json!({
              "members": [{
                "user_id": user_id,
                "endpoint_id": endpoint_id,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": "updated"
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
async fn delete_admin_grant_group_schedules_full_reconcile() {
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
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "test-group",
              "members": [{
                "user_id": user_id,
                "endpoint_id": endpoint_id,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
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
        .oneshot(req_authed("DELETE", "/api/admin/grant-groups/test-group"))
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
    let app = build_router(
        config,
        store,
        ReconcileHandle::noop(),
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
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
    let app = build_router(
        config,
        store.clone(),
        ReconcileHandle::noop(),
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
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

    let token = setup_subscription_fixtures(&app, &store)
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
async fn subscription_default_base64_matches_raw_and_content_type() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let token = setup_subscription_fixtures(&app, &store)
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

    assert_eq!(decoded_text, raw_body);
}

#[tokio::test]
async fn subscription_format_clash_returns_yaml_with_proxies() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let token = setup_subscription_fixtures(&app, &store)
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
async fn subscription_token_reset_invalidates_old_token() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&app, &store).await;
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
async fn subscription_disabled_grant_not_in_output() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let fixtures = setup_subscription_fixtures(&app, &store).await;
    let token = fixtures.subscription_token;
    let group_name = fixtures.group_name;
    let user_id = fixtures.user_id;
    let endpoint_id = fixtures.endpoint_id;
    let password = fixtures.ss2022_password;

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/grant-groups/{group_name}"),
            json!({
              "members": [{
                "user_id": user_id,
                "endpoint_id": endpoint_id,
                "enabled": false,
                "quota_limit_bytes": 0,
                "note": null
              }]
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
async fn grant_group_replace_clears_quota_ban_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let fixtures = setup_subscription_fixtures(&app, &store).await;
    let group_name = fixtures.group_name;
    let grant_id = fixtures.grant_id;
    let user_id = fixtures.user_id;
    let endpoint_id = fixtures.endpoint_id;
    let banned_at = "2025-12-18T00:00:00Z".to_string();

    {
        let mut store = store.lock().await;
        store.set_quota_banned(&grant_id, banned_at).unwrap();
        assert!(store.get_grant_usage(&grant_id).unwrap().quota_banned);
    }

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/grant-groups/{group_name}"),
            json!({
              "members": [{
                "user_id": user_id,
                "endpoint_id": endpoint_id,
                "enabled": false,
                "quota_limit_bytes": 0,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let store = store.lock().await;
    let usage = store.get_grant_usage(&grant_id).unwrap();
    assert!(!usage.quota_banned);
    assert_eq!(usage.quota_banned_at, None);
}

#[tokio::test]
async fn admin_alerts_local_reports_quota_mismatch() {
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
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "test-group",
              "members": [{
                "user_id": user_id,
                "endpoint_id": endpoint_id,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let grant_id = {
        let store = store.lock().await;
        store
            .list_grants()
            .into_iter()
            .find(|g| g.user_id == user_id && g.endpoint_id == endpoint_id)
            .map(|g| g.grant_id)
            .expect("expected grant to exist for alert fixture")
    };

    let banned_at = "2025-12-18T00:00:00Z".to_string();
    {
        let mut store = store.lock().await;
        store
            .set_quota_banned(&grant_id, banned_at.clone())
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
    assert_eq!(item["type"], "quota_enforced_but_desired_enabled");
    assert_eq!(item["grant_id"], grant_id);
    assert_eq!(item["endpoint_id"], endpoint_id);
    assert_eq!(item["owner_node_id"], node_id);
    assert_eq!(item["desired_enabled"], true);
    assert_eq!(item["quota_banned"], true);
    assert_eq!(item["quota_banned_at"], banned_at);
    assert_eq!(item["effective_enabled"], false);
    assert_eq!(
        item["message"],
        "quota enforced on owner node but desired state is still enabled"
    );
    assert_eq!(
        item["action_hint"],
        "check raft leader/quorum and retry status"
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

#[tokio::test]
async fn admin_delete_grant_removes_usage_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());

    let fixtures = setup_subscription_fixtures(&app, &store).await;
    let group_name = fixtures.group_name;
    let grant_id = fixtures.grant_id;

    {
        let mut store = store.lock().await;
        store
            .set_quota_banned(&grant_id, "2025-12-18T00:00:00Z".to_string())
            .unwrap();
        assert!(store.get_grant_usage(&grant_id).is_some());
    }

    let res = app
        .oneshot(req_authed(
            "DELETE",
            &format!("/api/admin/grant-groups/{group_name}"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let store = store.lock().await;
    assert!(store.get_grant_usage(&grant_id).is_none());
}

#[tokio::test]
async fn subscription_invalid_format_returns_400_invalid_request() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "example.com").await;

    let token = setup_subscription_fixtures(&app, &store)
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
    let app = build_router(
        config.clone(),
        store,
        crate::reconcile::ReconcileHandle::noop(),
        cluster.clone(),
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
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
    let app = build_router(
        config,
        store,
        crate::reconcile::ReconcileHandle::noop(),
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
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
async fn vless_endpoint_creation_persists_reality_materials_and_grant_uuid_is_uuidv4() {
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

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "test-group",
              "members": [{
                "user_id": user["user_id"],
                "endpoint_id": endpoint_id,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let group = body_json(res).await;
    let vless = &group["members"][0]["credentials"]["vless"];
    let uuid = vless["uuid"].as_str().unwrap();
    assert!(Uuid::parse_str(uuid).is_ok());
    assert!(!is_ulid_string(uuid));
    let email = vless["email"].as_str().unwrap();
    let Some((prefix, id)) = email.split_once(':') else {
        panic!("expected email to be in grant:<id> format, got {email}");
    };
    assert_eq!(prefix, "grant");
    assert!(is_ulid_string(id));
}

#[tokio::test]
async fn ss2022_endpoint_creation_persists_server_psk_and_grant_password_uses_server_and_user_psk()
{
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
    let endpoint_id = endpoint["endpoint_id"].as_str().unwrap();

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

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "test-group",
              "members": [{
                "user_id": user["user_id"],
                "endpoint_id": endpoint_id,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let group = body_json(res).await;
    let ss2022 = &group["members"][0]["credentials"]["ss2022"];
    assert_eq!(ss2022["method"], "2022-blake3-aes-128-gcm");

    let password = ss2022["password"].as_str().unwrap();
    let (server_part, user_part) = password.split_once(':').unwrap();
    assert_eq!(server_part, server_psk_b64);
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
