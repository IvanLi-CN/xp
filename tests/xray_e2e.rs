use std::{net::SocketAddr, path::PathBuf};

use axum::{body::Body, http::Request};
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::time::{Duration, Instant, sleep};
use tower::util::ServiceExt;

use xp::{config::Config, http::build_router, reconcile::spawn_reconciler, state::StoreInit, xray};

fn env_socket_addr(key: &str) -> Result<SocketAddr, String> {
    let raw = std::env::var(key)
        .map_err(|_| format!("missing env {key}; run via `scripts/e2e/run-local-xray-e2e.sh`"))?;
    raw.parse::<SocketAddr>()
        .map_err(|e| format!("invalid {key}={raw}: {e}"))
}

fn test_config(data_dir: PathBuf, xray_api_addr: SocketAddr) -> Config {
    Config {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        xray_api_addr,
        data_dir,
        admin_token: "testtoken".to_string(),
        node_name: "node-1".to_string(),
        public_domain: "".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        quota_poll_interval_secs: 10,
        quota_auto_unban: true,
    }
}

fn store_init(config: &Config) -> StoreInit {
    StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_name: config.node_name.clone(),
        bootstrap_public_domain: config.public_domain.clone(),
        bootstrap_api_base_url: config.api_base_url.clone(),
    }
}

fn req_authed_json(uri: &str, value: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(axum::http::header::AUTHORIZATION, "Bearer testtoken")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&value).unwrap()))
        .unwrap()
}

async fn wait_for_remove_user(client: &mut xray::XrayClient, tag: &str, email: &str) {
    use xp::xray::proto::xray::app::proxyman::command::AlterInboundRequest;

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let req = AlterInboundRequest {
            tag: tag.to_string(),
            operation: Some(xp::xray::builder::build_remove_user_operation(email)),
        };
        match client.alter_inbound(req).await {
            Ok(_) => return,
            Err(status) if xray::is_not_found(&status) => {
                if Instant::now() >= deadline {
                    panic!("timeout waiting for xray user to exist: tag={tag} email={email}");
                }
                sleep(Duration::from_millis(100)).await;
            }
            Err(status) => panic!("xray alter_inbound remove_user failed: {status}"),
        }
    }
}

async fn wait_for_remove_inbound(client: &mut xray::XrayClient, tag: &str) {
    use xp::xray::proto::xray::app::proxyman::command::RemoveInboundRequest;

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let req = RemoveInboundRequest {
            tag: tag.to_string(),
        };
        match client.remove_inbound(req).await {
            Ok(_) => return,
            Err(status) if xray::is_not_found(&status) => {
                if Instant::now() >= deadline {
                    panic!("timeout waiting for xray inbound to exist: tag={tag}");
                }
                sleep(Duration::from_millis(100)).await;
            }
            Err(status) => panic!("xray remove_inbound failed: {status}"),
        }
    }
}

#[tokio::test]
#[ignore]
async fn xray_e2e_apply_endpoints_and_grants_via_reconcile() {
    if std::env::var("XP_E2E_XRAY_MODE").ok().as_deref() != Some("external") {
        return;
    }

    let xray_api_addr = env_socket_addr("XP_E2E_XRAY_API_ADDR").unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path().to_path_buf(), xray_api_addr);

    let store = xp::state::JsonSnapshotStore::load_or_init(store_init(&config)).unwrap();
    let store = std::sync::Arc::new(tokio::sync::Mutex::new(store));
    let reconcile = spawn_reconciler(std::sync::Arc::new(config.clone()), store.clone());
    let app = build_router(config, store.clone(), reconcile);

    let node_id = { store.lock().await.list_nodes()[0].node_id.clone() };

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "/api/admin/users",
            json!({
              "display_name": "alice",
              "cycle_policy_default": "by_user",
              "cycle_day_of_month_default": 1
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let user: serde_json::Value = {
        use http_body_util::BodyExt as _;
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    };
    let user_id = user["user_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 31080
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let endpoint_ss: serde_json::Value = {
        use http_body_util::BodyExt as _;
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    };
    let endpoint_id_ss = endpoint_ss["endpoint_id"].as_str().unwrap().to_string();
    let endpoint_tag_ss = endpoint_ss["tag"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "/api/admin/endpoints",
            json!({
              "node_id": endpoint_ss["node_id"],
              "kind": "vless_reality_vision_tcp",
              "port": 31081,
              "public_domain": "example.com",
              "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let endpoint_vless: serde_json::Value = {
        use http_body_util::BodyExt as _;
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    };
    let endpoint_id_vless = endpoint_vless["endpoint_id"].as_str().unwrap().to_string();
    let endpoint_tag_vless = endpoint_vless["tag"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "/api/admin/grants",
            json!({
              "user_id": user_id,
              "endpoint_id": endpoint_id_ss,
              "quota_limit_bytes": 0,
              "cycle_policy": "inherit_user",
              "cycle_day_of_month": null,
              "note": null
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let grant_ss: serde_json::Value = {
        use http_body_util::BodyExt as _;
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    };
    let grant_id_ss = grant_ss["grant_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "/api/admin/grants",
            json!({
              "user_id": user_id,
              "endpoint_id": endpoint_id_vless,
              "quota_limit_bytes": 0,
              "cycle_policy": "inherit_user",
              "cycle_day_of_month": null,
              "note": null
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let grant_vless: serde_json::Value = {
        use http_body_util::BodyExt as _;
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    };
    let grant_id_vless = grant_vless["grant_id"].as_str().unwrap().to_string();

    let mut client = xray::connect(xray_api_addr).await.unwrap();
    wait_for_remove_user(
        &mut client,
        &endpoint_tag_ss,
        &format!("grant:{grant_id_ss}"),
    )
    .await;
    wait_for_remove_user(
        &mut client,
        &endpoint_tag_vless,
        &format!("grant:{grant_id_vless}"),
    )
    .await;

    wait_for_remove_inbound(&mut client, &endpoint_tag_ss).await;
    wait_for_remove_inbound(&mut client, &endpoint_tag_vless).await;
}
