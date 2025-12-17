use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use bytes::Bytes;
use http_body_util::BodyExt;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::util::ServiceExt;

use crate::{
    config::Config,
    http::build_router,
    id::{is_ulid_string, new_ulid_string},
    state::{JsonSnapshotStore, StoreInit},
};

fn test_config(data_dir: PathBuf) -> Config {
    Config {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir,
        admin_token: "testtoken".to_string(),
        node_name: "node-1".to_string(),
        public_domain: "".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
    }
}

fn test_store_init(config: &Config) -> StoreInit {
    StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_name: config.node_name.clone(),
        bootstrap_public_domain: config.public_domain.clone(),
        bootstrap_api_base_url: config.api_base_url.clone(),
    }
}

fn app(tmp: &TempDir) -> axum::Router {
    let config = test_config(tmp.path().to_path_buf());
    let store = JsonSnapshotStore::load_or_init(test_store_init(&config)).unwrap();
    build_router(config, Arc::new(Mutex::new(store)))
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

async fn body_bytes(res: axum::response::Response) -> Bytes {
    res.into_body().collect().await.unwrap().to_bytes()
}

async fn body_json(res: axum::response::Response) -> Value {
    let bytes = body_bytes(res).await;
    serde_json::from_slice(&bytes).unwrap()
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
async fn cluster_info_is_single_node_leader_and_ids_present() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app.oneshot(req("GET", "/api/cluster/info")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;

    let node_id = json["node_id"].as_str().unwrap();
    let cluster_id = json["cluster_id"].as_str().unwrap();
    assert!(is_ulid_string(node_id));
    assert_eq!(cluster_id, node_id);
    assert_eq!(json["role"], "leader");
    assert_eq!(json["term"], 1);
    assert_eq!(json["leader_api_base_url"], "https://127.0.0.1:62416");
}

#[tokio::test]
async fn create_user_then_list_contains_it() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let create = req_authed_json(
        "POST",
        "/api/admin/users",
        json!({
          "display_name": "alice",
          "cycle_policy_default": "by_user",
          "cycle_day_of_month_default": 1
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
async fn create_grant_with_missing_resources_returns_404_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let create = req_authed_json(
        "POST",
        "/api/admin/grants",
        json!({
          "user_id": new_ulid_string(),
          "endpoint_id": new_ulid_string(),
          "quota_limit_bytes": 0,
          "cycle_policy": "inherit_user",
          "cycle_day_of_month": null,
          "note": null
        }),
    );
    let res = app.oneshot(create).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_found");
}

#[tokio::test]
async fn patch_grant_validates_cycle_day_of_month_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

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
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/grants/{grant_id}"),
            json!({
              "enabled": true,
              "quota_limit_bytes": 0,
              "cycle_policy": "by_user",
              "cycle_day_of_month": null
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn grant_usage_is_501_not_implemented() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

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
    assert_eq!(res.status(), StatusCode::NOT_IMPLEMENTED);
    let json = body_json(res).await;
    assert_eq!(json["error"]["code"], "not_implemented");
}

#[tokio::test]
async fn persistence_smoke_user_roundtrip_via_api() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();

    let config = test_config(data_dir.clone());
    let store = JsonSnapshotStore::load_or_init(test_store_init(&config)).unwrap();
    let app = build_router(config.clone(), Arc::new(Mutex::new(store)));

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

    let store = JsonSnapshotStore::load_or_init(test_store_init(&config)).unwrap();
    let app = build_router(config, Arc::new(Mutex::new(store)));

    let res = app
        .oneshot(req_authed("GET", "/api/admin/users"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_json(res).await;
    let items = listed["items"].as_array().unwrap();
    assert!(items.iter().any(|u| u["user_id"] == user_id));
}
