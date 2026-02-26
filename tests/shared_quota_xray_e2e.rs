use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

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
    domain::UserPriorityTier,
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
        .map_err(|_| format!("missing env {key}; run via testbox runner script"))?;
    raw.parse::<SocketAddr>()
        .map_err(|e| format!("invalid {key}={raw}: {e}"))
}

fn env_u16(key: &str) -> Result<u16, String> {
    let raw = std::env::var(key)
        .map_err(|_| format!("missing env {key}; run via testbox runner script"))?;
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
        cloudflared_health_interval_secs: 5,
        cloudflared_health_fails_before_down: 3,
        cloudflared_restart_mode: xp::config::XrayRestartMode::None,
        cloudflared_restart_cooldown_secs: 30,
        cloudflared_restart_timeout_secs: 5,
        cloudflared_systemd_unit: "cloudflared.service".to_string(),
        cloudflared_openrc_service: "cloudflared".to_string(),
        data_dir,
        admin_token_hash: test_admin_token_hash("testtoken"),
        node_name: "node-1".to_string(),
        access_host: "".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        endpoint_probe_skip_self_test: false,
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

fn req_authed_json(method: &str, uri: &str, value: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(axum::http::header::AUTHORIZATION, "Bearer testtoken")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&value).unwrap()))
        .unwrap()
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

async fn wait_for_ss_ok(ss_port: u16, password: &str, dest_port: u16, payload_len: usize) {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        match ss_roundtrip_echo(ss_port, password, dest_port, payload_len).await {
            Ok(()) => return,
            Err(err) => {
                if Instant::now() >= deadline {
                    panic!("timeout waiting for ss to become ready: {err}");
                }
                sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

async fn wait_for_ss_fail(ss_port: u16, password: &str, dest_port: u16) {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        match ss_roundtrip_echo(ss_port, password, dest_port, 64).await {
            Ok(()) => {
                if Instant::now() >= deadline {
                    panic!("expected ss to fail, but it kept succeeding until deadline");
                }
                sleep(Duration::from_millis(200)).await;
            }
            Err(_) => return,
        }
    }
}

#[tokio::test]
#[ignore]
async fn shared_quota_e2e_p3_is_banned_without_overflow_then_unbanned_with_overflow() {
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
    let cloudflared_health = xp::cloudflared_supervisor::CloudflaredHealthHandle::new_with_status(
        xp::cloudflared_supervisor::CloudflaredStatus::Disabled,
    );
    let (node_runtime, _node_runtime_task) = xp::node_runtime::spawn_node_runtime_monitor(
        std::sync::Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );
    let endpoint_probe = xp::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let app = build_router(
        config.clone(),
        store.clone(),
        reconcile.clone(),
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft.clone(),
        None,
    );

    let node_id = { store.lock().await.list_nodes()[0].node_id.clone() };

    // Enable shared-quota on the node (monthly reset with a fixed UTC offset).
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/nodes/{node_id}"),
            json!({
              "quota_limit_bytes": 1024 * 1024 * 1024, // 1GiB
              "quota_reset": { "policy": "monthly", "day_of_month": 1, "tz_offset_minutes": 0 }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);

    // Create users P1/P2/P3.
    let mut p1_user_id = None::<String>;
    let mut p2_user_id = None::<String>;
    let mut p3_user_id = None::<String>;
    for (tier, name) in [
        (UserPriorityTier::P1, "p1"),
        (UserPriorityTier::P2, "p2"),
        (UserPriorityTier::P3, "p3"),
    ] {
        let res = app
            .clone()
            .oneshot(req_authed_json(
                "POST",
                "/api/admin/users",
                json!({ "display_name": name }),
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

        let tier_str = match tier {
            UserPriorityTier::P1 => "p1",
            UserPriorityTier::P2 => "p2",
            UserPriorityTier::P3 => "p3",
        };
        let res = app
            .clone()
            .oneshot(req_authed_json(
                "PATCH",
                &format!("/api/admin/users/{user_id}"),
                json!({ "priority_tier": tier_str }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);

        match tier {
            UserPriorityTier::P1 => p1_user_id = Some(user_id),
            UserPriorityTier::P2 => p2_user_id = Some(user_id),
            UserPriorityTier::P3 => p3_user_id = Some(user_id),
        }
    }
    let p1_user_id = p1_user_id.unwrap();
    let p2_user_id = p2_user_id.unwrap();
    let p3_user_id = p3_user_id.unwrap();

    // Create a single SS endpoint on the forwarded port, shared by all users.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
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
    let endpoint_id_ss = endpoint_ss["endpoint_id"].as_str().unwrap().to_string();
    let endpoint_tag_ss = endpoint_ss["tag"].as_str().unwrap().to_string();

    // Create a grant group: P1/P2/P3 all enabled.
    let members = [p1_user_id.clone(), p2_user_id.clone(), p3_user_id.clone()]
        .into_iter()
        .map(|user_id| {
            json!({
              "user_id": user_id,
              "endpoint_id": endpoint_id_ss,
              "enabled": true,
              "quota_limit_bytes": 0,
              "note": null
            })
        })
        .collect::<Vec<_>>();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "shared-quota-e2e-group",
              "members": members
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

    let mut ss_password_by_user_id = BTreeMap::<String, String>::new();
    for member in group_detail["members"].as_array().unwrap() {
        let user_id = member["user_id"].as_str().unwrap().to_string();
        let password = member["credentials"]["ss2022"]["password"]
            .as_str()
            .unwrap()
            .to_string();
        ss_password_by_user_id.insert(user_id, password);
    }

    let p1_password = ss_password_by_user_id.get(&p1_user_id).unwrap().clone();
    let p3_password = ss_password_by_user_id.get(&p3_user_id).unwrap().clone();

    let (dest_port, echo_task) = spawn_echo_server().await;

    // Ensure the inbound is applied and at least one user can connect (tiny traffic).
    wait_for_ss_ok(ss_port, &p1_password, dest_port, 128).await;

    // Tick day 0: P3 has no base bank and should be banned immediately.
    let now0 = chrono::DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    xp::quota::run_quota_tick_at(now0, &config, &store, &reconcile, &raft)
        .await
        .unwrap();

    // Wait for P3 to be denied.
    wait_for_ss_fail(ss_port, &p3_password, dest_port).await;

    // Tick day 2: P2's pacing overflow should flow to P1 bonus and then overflow to P3.
    // This should unban P3 even without any additional traffic.
    let now2 = chrono::DateTime::parse_from_rfc3339("2026-02-03T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    xp::quota::run_quota_tick_at(now2, &config, &store, &reconcile, &raft)
        .await
        .unwrap();

    // Wait for P3 to become usable again.
    wait_for_ss_ok(ss_port, &p3_password, dest_port, 128).await;

    echo_task.abort();

    use xp::xray::proto::xray::app::proxyman::command::RemoveInboundRequest;
    let mut client = xray::connect(xray_api_addr).await.unwrap();
    let _ = client
        .remove_inbound(RemoveInboundRequest {
            tag: endpoint_tag_ss,
        })
        .await;
}

#[tokio::test]
#[ignore]
async fn shared_quota_e2e_policy_change_weight_decrease_bans_without_new_traffic() {
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
    let cloudflared_health = xp::cloudflared_supervisor::CloudflaredHealthHandle::new_with_status(
        xp::cloudflared_supervisor::CloudflaredStatus::Disabled,
    );
    let (node_runtime, _node_runtime_task) = xp::node_runtime::spawn_node_runtime_monitor(
        std::sync::Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );
    let endpoint_probe = xp::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let app = build_router(
        config.clone(),
        store.clone(),
        reconcile.clone(),
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft.clone(),
        None,
    );

    let node_id = { store.lock().await.list_nodes()[0].node_id.clone() };

    // Enable shared-quota on the node.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/nodes/{node_id}"),
            json!({
              "quota_limit_bytes": 1024 * 1024 * 1024, // 1GiB
              "quota_reset": { "policy": "monthly", "day_of_month": 1, "tz_offset_minutes": 0 }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);

    // Create users P1 + P2 (P2 will be banned after weight drops to 0).
    let p1_user_id = {
        let res = app
            .clone()
            .oneshot(req_authed_json(
                "POST",
                "/api/admin/users",
                json!({ "display_name": "p1" }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let user: serde_json::Value = {
            use http_body_util::BodyExt as _;
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice(&bytes).unwrap()
        };
        user["user_id"].as_str().unwrap().to_string()
    };
    let p2_user_id = {
        let res = app
            .clone()
            .oneshot(req_authed_json(
                "POST",
                "/api/admin/users",
                json!({ "display_name": "p2" }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let user: serde_json::Value = {
            use http_body_util::BodyExt as _;
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice(&bytes).unwrap()
        };
        user["user_id"].as_str().unwrap().to_string()
    };

    for (user_id, tier) in [(&p1_user_id, "p1"), (&p2_user_id, "p2")] {
        let res = app
            .clone()
            .oneshot(req_authed_json(
                "PATCH",
                &format!("/api/admin/users/{user_id}"),
                json!({ "priority_tier": tier }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
    }

    // Single SS endpoint on forwarded port.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
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
    let endpoint_id_ss = endpoint_ss["endpoint_id"].as_str().unwrap().to_string();
    let endpoint_tag_ss = endpoint_ss["tag"].as_str().unwrap().to_string();

    // Grant group.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "shared-quota-weight-drop",
              "members": [{
                "user_id": p1_user_id.clone(),
                "endpoint_id": endpoint_id_ss,
                "enabled": true,
                "quota_limit_bytes": 0,
                "note": null
              }, {
                "user_id": p2_user_id.clone(),
                "endpoint_id": endpoint_id_ss,
                "enabled": true,
                "quota_limit_bytes": 0,
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

    let mut p2_password = None;
    for member in group_detail["members"].as_array().unwrap() {
        if member["user_id"].as_str().unwrap() == p2_user_id {
            p2_password = Some(
                member["credentials"]["ss2022"]["password"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
        }
    }
    let p2_password = p2_password.expect("expected p2 password");

    let (dest_port, echo_task) = spawn_echo_server().await;

    // Ensure inbound is ready, and generate a small amount of P2 traffic.
    wait_for_ss_ok(ss_port, &p2_password, dest_port, 16 * 1024).await;

    let now0 = chrono::DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    xp::quota::run_quota_tick_at(now0, &config, &store, &reconcile, &raft)
        .await
        .unwrap();

    // Drop P2's weight to 0: base share becomes 0. Because it already consumed some traffic
    // earlier in the same day, this should trigger an immediate ban without new traffic.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PUT",
            &format!("/api/admin/users/{p2_user_id}/node-weights/{node_id}"),
            json!({ "weight": 0 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);

    let now0_later = now0 + chrono::Duration::hours(1);
    xp::quota::run_quota_tick_at(now0_later, &config, &store, &reconcile, &raft)
        .await
        .unwrap();

    // P2 should now be denied.
    wait_for_ss_fail(ss_port, &p2_password, dest_port).await;

    echo_task.abort();

    use xp::xray::proto::xray::app::proxyman::command::RemoveInboundRequest;
    let mut client = xray::connect(xray_api_addr).await.unwrap();
    let _ = client
        .remove_inbound(RemoveInboundRequest {
            tag: endpoint_tag_ss,
        })
        .await;
}

#[tokio::test]
#[ignore]
async fn shared_quota_e2e_cycle_rollover_unbans_and_resets() {
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
    let cloudflared_health = xp::cloudflared_supervisor::CloudflaredHealthHandle::new_with_status(
        xp::cloudflared_supervisor::CloudflaredStatus::Disabled,
    );
    let (node_runtime, _node_runtime_task) = xp::node_runtime::spawn_node_runtime_monitor(
        std::sync::Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );
    let endpoint_probe = xp::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let app = build_router(
        config.clone(),
        store.clone(),
        reconcile.clone(),
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft.clone(),
        None,
    );

    let node_id = { store.lock().await.list_nodes()[0].node_id.clone() };

    // Enable shared-quota with a small-but-nonzero distributable budget (fast ban).
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/nodes/{node_id}"),
            json!({
              "quota_limit_bytes": 256 * 1024 * 1024 + 64 * 1024 * 1024, // buffer(256MiB) + 64MiB
              "quota_reset": { "policy": "monthly", "day_of_month": 1, "tz_offset_minutes": 0 }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);

    // Create P2 user.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({ "display_name": "p2" }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let user: serde_json::Value = {
        use http_body_util::BodyExt as _;
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    };
    let p2_user_id = user["user_id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/users/{p2_user_id}"),
            json!({ "priority_tier": "p2" }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);

    // Single SS endpoint on forwarded port.
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
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
    let endpoint_id_ss = endpoint_ss["endpoint_id"].as_str().unwrap().to_string();
    let endpoint_tag_ss = endpoint_ss["tag"].as_str().unwrap().to_string();

    // Grant group (single member).
    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/grant-groups",
            json!({
              "group_name": "shared-quota-rollover",
              "members": [{
                "user_id": p2_user_id,
                "endpoint_id": endpoint_id_ss,
                "enabled": true,
                "quota_limit_bytes": 0,
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
    let p2_password = group_detail["members"][0]["credentials"]["ss2022"]["password"]
        .as_str()
        .unwrap()
        .to_string();

    let (dest_port, echo_task) = spawn_echo_server().await;

    // Wait until SS is ready, then send enough traffic to exceed the initial day0 bank.
    wait_for_ss_ok(ss_port, &p2_password, dest_port, 256).await;
    // Force a ban: exceed the initial day0 bank. Payload is counted in both uplink + downlink.
    ss_roundtrip_echo(ss_port, &p2_password, dest_port, 4 * 1024 * 1024)
        .await
        .unwrap();

    let now0 = chrono::DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    xp::quota::run_quota_tick_at(now0, &config, &store, &reconcile, &raft)
        .await
        .unwrap();

    wait_for_ss_fail(ss_port, &p2_password, dest_port).await;

    // Jump to the next cycle: bans should be cleared and pacing reset, restoring connectivity.
    let now_future = now0 + chrono::Duration::days(40);
    xp::quota::run_quota_tick_at(now_future, &config, &store, &reconcile, &raft)
        .await
        .unwrap();

    wait_for_ss_ok(ss_port, &p2_password, dest_port, 128).await;

    echo_task.abort();

    use xp::xray::proto::xray::app::proxyman::command::RemoveInboundRequest;
    let mut client = xray::connect(xray_api_addr).await.unwrap();
    let _ = client
        .remove_inbound(RemoveInboundRequest {
            tag: endpoint_tag_ss,
        })
        .await;
}
