use std::{net::SocketAddr, path::PathBuf};

use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use axum::{body::Body, http::Request};
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::{Duration, Instant, sleep},
};
use tower::util::ServiceExt;

use xp::{
    cluster_metadata::ClusterMetadata,
    raft::{
        app::LocalRaft,
        types::{NodeMeta as RaftNodeMeta, raft_node_id_from_ulid},
    },
    xray_supervisor::XrayHealthHandle,
};
use xp::{config::Config, http::build_router, reconcile::spawn_reconciler, state::StoreInit, xray};

fn test_admin_token_hash(token: &str) -> String {
    // Fast + deterministic: keep integration tests snappy.
    let params = Params::new(32, 1, 1, None).expect("argon2 params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let salt = SaltString::encode_b64(b"xp-test-salt").expect("salt");
    argon2
        .hash_password(token.as_bytes(), &salt)
        .expect("hash_password")
        .to_string()
}

fn env_socket_addr(key: &str) -> Result<SocketAddr, String> {
    let raw = std::env::var(key)
        .map_err(|_| format!("missing env {key}; run via `scripts/e2e/run-local-xray-e2e.sh`"))?;
    raw.parse::<SocketAddr>()
        .map_err(|e| format!("invalid {key}={raw}: {e}"))
}

fn env_u16(key: &str) -> Result<u16, String> {
    let raw = std::env::var(key)
        .map_err(|_| format!("missing env {key}; run via `scripts/e2e/run-local-xray-e2e.sh`"))?;
    raw.parse::<u16>()
        .map_err(|e| format!("invalid {key}={raw}: {e}"))
}

fn test_config(data_dir: PathBuf, xray_api_addr: SocketAddr) -> Config {
    Config {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        xray_api_addr,
        xray_health_interval_secs: 2,
        xray_health_fails_before_down: 3,
        xray_restart_mode: xp::config::XrayRestartMode::None,
        xray_restart_cooldown_secs: 30,
        xray_restart_timeout_secs: 5,
        xray_systemd_unit: "xray.service".to_string(),
        xray_openrc_service: "xray".to_string(),
        data_dir,
        admin_token_hash: test_admin_token_hash("testtoken"),
        node_name: "node-1".to_string(),
        access_host: "".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        quota_poll_interval_secs: 10,
        quota_auto_unban: true,
    }
}

fn store_init(config: &Config, bootstrap_node_id: Option<String>) -> StoreInit {
    StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id,
        bootstrap_node_name: config.node_name.clone(),
        bootstrap_access_host: config.access_host.clone(),
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

async fn spawn_echo_server() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind(("0.0.0.0", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let handle = tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    let n = match stream.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    if stream.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                let _ = stream.shutdown().await;
            });
        }
    });

    (port, handle)
}

async fn ss_roundtrip_echo(
    ss_port: u16,
    password: &str,
    dest_port: u16,
    payload_len: usize,
) -> std::io::Result<()> {
    use shadowsocks::{
        config::{ServerConfig, ServerType},
        context::Context,
        crypto::CipherKind,
        relay::tcprelay::proxy_stream::ProxyClientStream,
    };

    let context = Context::new_shared(ServerType::Local);
    let server_addr = SocketAddr::from(([127, 0, 0, 1], ss_port));
    let server_cfg = ServerConfig::new(
        server_addr,
        password.to_string(),
        CipherKind::AEAD2022_BLAKE3_AES_128_GCM,
    )
    .map_err(std::io::Error::other)?;

    let mut stream = ProxyClientStream::connect(
        context,
        &server_cfg,
        ("host.docker.internal".to_string(), dest_port),
    )
    .await?;

    let payload = vec![0x42; payload_len];
    // ProxyClientStream's first write includes handshake+payload in one buffer and
    // expects the encrypted write to behave like write_all in debug builds.
    // Keep the first write small to avoid triggering internal debug assertions.
    let first_chunk_len = payload.len().min(1024);
    stream.write_all(&payload[..first_chunk_len]).await?;
    stream.write_all(&payload[first_chunk_len..]).await?;
    stream.flush().await?;

    let mut received = vec![0u8; payload.len()];
    stream.read_exact(&mut received).await?;
    assert_eq!(received, payload);

    stream.shutdown().await?;
    Ok(())
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

    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(tmp.path()).unwrap();

    let store = xp::state::JsonSnapshotStore::load_or_init(store_init(
        &config,
        Some(cluster.node_id.clone()),
    ))
    .unwrap();
    let store = std::sync::Arc::new(tokio::sync::Mutex::new(store));
    let reconcile = spawn_reconciler(std::sync::Arc::new(config.clone()), store.clone());

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
    metrics.membership_config =
        std::sync::Arc::new(openraft::StoredMembership::new(None, membership));
    let (_tx, rx) = tokio::sync::watch::channel(metrics);
    let raft: std::sync::Arc<dyn xp::raft::app::RaftFacade> =
        std::sync::Arc::new(LocalRaft::new(store.clone(), rx));

    let xray_health = XrayHealthHandle::new_unknown();
    let endpoint_probe = xp::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
    );
    let app = build_router(
        config,
        store.clone(),
        reconcile,
        xray_health,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft.clone(),
        None,
    );

    let node_id = { store.lock().await.list_nodes()[0].node_id.clone() };

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "/api/admin/users",
            json!({
              "display_name": "alice",
              "quota_reset": {
                "policy": "monthly",
                "day_of_month": 1,
                "tz_offset_minutes": 480
              }
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
            "/api/admin/grant-groups",
            json!({
              "group_name": "xray-e2e-group",
              "members": [{
                "user_id": user_id,
                "endpoint_id": endpoint_id_ss,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
              }, {
                "user_id": user_id,
                "endpoint_id": endpoint_id_vless,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let _group_detail: serde_json::Value = {
        use http_body_util::BodyExt as _;
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    };

    let grant_id_ss = {
        let store = store.lock().await;
        store
            .list_grants()
            .into_iter()
            .find(|g| g.user_id == user_id && g.endpoint_id == endpoint_id_ss)
            .map(|g| g.grant_id)
            .expect("expected grant to exist for ss endpoint")
    };

    let grant_id_vless = {
        let store = store.lock().await;
        store
            .list_grants()
            .into_iter()
            .find(|g| g.user_id == user_id && g.endpoint_id == endpoint_id_vless)
            .map(|g| g.grant_id)
            .expect("expected grant to exist for vless endpoint")
    };

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

#[tokio::test]
#[ignore]
async fn xray_e2e_quota_enforcement_ss2022() {
    if std::env::var("XP_E2E_XRAY_MODE").ok().as_deref() != Some("external") {
        return;
    }

    let xray_api_addr = env_socket_addr("XP_E2E_XRAY_API_ADDR").unwrap();
    let ss_port = env_u16("XP_E2E_SS_PORT").unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path().to_path_buf(), xray_api_addr);

    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(tmp.path()).unwrap();

    let store = xp::state::JsonSnapshotStore::load_or_init(store_init(
        &config,
        Some(cluster.node_id.clone()),
    ))
    .unwrap();
    let store = std::sync::Arc::new(tokio::sync::Mutex::new(store));
    let reconcile = spawn_reconciler(std::sync::Arc::new(config.clone()), store.clone());

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
    metrics.membership_config =
        std::sync::Arc::new(openraft::StoredMembership::new(None, membership));
    let (_tx, rx) = tokio::sync::watch::channel(metrics);
    let raft: std::sync::Arc<dyn xp::raft::app::RaftFacade> =
        std::sync::Arc::new(LocalRaft::new(store.clone(), rx));

    let xray_health = XrayHealthHandle::new_unknown();
    let endpoint_probe = xp::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
    );
    let app = build_router(
        config.clone(),
        store.clone(),
        reconcile.clone(),
        xray_health,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft.clone(),
        None,
    );

    let node_id = { store.lock().await.list_nodes()[0].node_id.clone() };

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "/api/admin/users",
            json!({
              "display_name": "quota-e2e",
              "quota_reset": {
                "policy": "monthly",
                "day_of_month": 1,
                "tz_offset_minutes": 480
              }
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
              "port": ss_port
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
    let endpoint_tag_ss = endpoint_ss["tag"].as_str().unwrap().to_string();
    let endpoint_id_ss = endpoint_ss["endpoint_id"].as_str().unwrap().to_string();

    let quota_limit_bytes: u64 = 12 * 1024 * 1024;
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "/api/admin/grant-groups",
            json!({
              "group_name": "quota-e2e-group",
              "members": [{
                "user_id": user_id,
                "endpoint_id": endpoint_id_ss,
                "enabled": true,
                "quota_limit_bytes": quota_limit_bytes,
                "note": null
              }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let group_detail: serde_json::Value = {
        use http_body_util::BodyExt as _;
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    };
    let ss_password = group_detail["members"][0]["credentials"]["ss2022"]["password"]
        .as_str()
        .unwrap()
        .to_string();

    let grant_id_ss = {
        let store = store.lock().await;
        store
            .list_grants()
            .into_iter()
            .find(|g| g.user_id == user_id && g.endpoint_id == endpoint_id_ss)
            .map(|g| g.grant_id)
            .expect("expected grant to exist for ss endpoint")
    };

    let (dest_port, echo_task) = spawn_echo_server().await;

    let payload_len = 1024 * 1024;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match ss_roundtrip_echo(ss_port, &ss_password, dest_port, payload_len).await {
            Ok(()) => break,
            Err(err) => {
                if Instant::now() >= deadline {
                    panic!("timeout waiting for ss inbound to become ready: {err}");
                }
                sleep(Duration::from_millis(200)).await;
            }
        }
    }

    let now = chrono::Utc::now();
    xp::quota::run_quota_tick_at(now, &config, &store, &reconcile, &raft)
        .await
        .unwrap();

    {
        let store = store.lock().await;
        let grant = store.get_grant(&grant_id_ss).unwrap();
        let usage = store.get_grant_usage(&grant_id_ss).unwrap();
        assert_eq!(grant.enabled, false);
        assert_eq!(usage.quota_banned, true);
    }

    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match ss_roundtrip_echo(ss_port, &ss_password, dest_port, 32).await {
            Ok(()) => {
                if Instant::now() >= deadline {
                    panic!("expected ss connection to fail after quota ban");
                }
                sleep(Duration::from_millis(200)).await;
            }
            Err(_) => break,
        }
    }

    let later = now + chrono::Duration::days(40);
    xp::quota::run_quota_tick_at(later, &config, &store, &reconcile, &raft)
        .await
        .unwrap();

    {
        let store = store.lock().await;
        let grant = store.get_grant(&grant_id_ss).unwrap();
        let usage = store.get_grant_usage(&grant_id_ss).unwrap();
        assert_eq!(grant.enabled, true);
        assert_eq!(usage.quota_banned, false);
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match ss_roundtrip_echo(ss_port, &ss_password, dest_port, 128).await {
            Ok(()) => break,
            Err(err) => {
                if Instant::now() >= deadline {
                    panic!("timeout waiting for ss inbound to be re-enabled: {err}");
                }
                sleep(Duration::from_millis(200)).await;
            }
        }
    }

    echo_task.abort();

    use xp::xray::proto::xray::app::proxyman::command::RemoveInboundRequest;
    let mut client = xray::connect(xray_api_addr).await.unwrap();
    let _ = client
        .remove_inbound(RemoveInboundRequest {
            tag: endpoint_tag_ss,
        })
        .await;
}
