use std::{net::SocketAddr, path::Path, sync::Arc};

use anyhow::Context as _;
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot, watch},
    task::JoinHandle,
    time::{Duration, Instant},
};

use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use xp::{
    cluster_metadata::ClusterMetadata,
    config::Config,
    domain::{User, UserQuotaReset},
    http::build_router,
    id::new_ulid_string,
    raft::{
        NodeId, NodeMeta,
        app::{ForwardingRaftFacade, RaftFacade as _},
        http_rpc::{RaftRpcState, build_raft_rpc_router},
        network_http::HttpNetworkFactory,
        runtime::start_raft,
        types::{ClientResponse, TypeConfig},
    },
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
};

struct ServerHandle {
    base_url: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: JoinHandle<anyhow::Result<()>>,
}

impl ServerHandle {
    async fn shutdown(mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.join
            .await
            .context("join server task")?
            .context("server exited with error")?;
        Ok(())
    }
}

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

async fn spawn_server(listener: TcpListener, router: axum::Router) -> anyhow::Result<ServerHandle> {
    let addr = listener.local_addr().context("server local_addr")?;
    let base_url = format!("http://{addr}");

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(|e| anyhow::anyhow!("axum serve: {e}"))?;
        Ok(())
    });

    Ok(ServerHandle {
        base_url,
        shutdown_tx: Some(shutdown_tx),
        join,
    })
}

async fn spawn_raft_rpc_server(raft: openraft::Raft<TypeConfig>) -> anyhow::Result<ServerHandle> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("bind raft rpc listener")?;
    let router = build_raft_rpc_router(RaftRpcState { raft });
    spawn_server(listener, router).await
}

fn store_init(
    data_dir: &Path,
    bootstrap_node_id: Option<String>,
    node_name: &str,
    api_base_url: &str,
) -> StoreInit {
    StoreInit {
        data_dir: data_dir.to_path_buf(),
        bootstrap_node_id,
        bootstrap_node_name: node_name.to_string(),
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: api_base_url.to_string(),
    }
}

async fn wait_for_leader(
    mut rx: watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
    expected_leader: NodeId,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let m = rx.borrow();
            if m.state == openraft::ServerState::Leader && m.current_leader == Some(expected_leader)
            {
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            let m = rx.borrow();
            anyhow::bail!(
                "timeout waiting for leader={expected_leader}; state={:?} current_leader={:?}",
                m.state,
                m.current_leader
            );
        }

        rx.changed().await.context("metrics changed")?;
    }
}

async fn wait_for_leader_base_url(
    mut rx: watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
    expected_leader: NodeId,
    timeout: Duration,
) -> anyhow::Result<String> {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let m = rx.borrow();
            if m.current_leader == Some(expected_leader) {
                if let Some((_, node)) = m
                    .membership_config
                    .nodes()
                    .find(|(id, _)| **id == expected_leader)
                {
                    if !node.api_base_url.is_empty() {
                        return Ok(node.api_base_url.clone());
                    }
                }
            }
        }

        if Instant::now() >= deadline {
            let m = rx.borrow();
            anyhow::bail!(
                "timeout waiting for leader base_url; leader={expected_leader} current_leader={:?} membership={}",
                m.current_leader,
                m.membership_config
            );
        }

        rx.changed().await.context("metrics changed")?;
    }
}

async fn wait_for_user(
    store: &Arc<Mutex<JsonSnapshotStore>>,
    user_id: &str,
    timeout: Duration,
) -> anyhow::Result<User> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(user) = { store.lock().await.get_user(user_id) } {
            return Ok(user);
        }
        if Instant::now() >= deadline {
            anyhow::bail!("timeout waiting for user_id={user_id}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn forwarding_raft_facade_client_write_forwards_to_leader() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir().context("tempdir")?;
    let leader_dir = tmp.path().join("leader");
    let follower_dir = tmp.path().join("follower");
    std::fs::create_dir_all(&leader_dir).context("create leader dir")?;
    std::fs::create_dir_all(&follower_dir).context("create follower dir")?;

    let admin_listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("bind admin listener")?;
    let admin_addr = admin_listener.local_addr().context("admin local_addr")?;
    let admin_base_url = format!("http://{admin_addr}");

    let admin_token = "testtoken".to_string();

    let cluster = ClusterMetadata::init_new_cluster(
        &leader_dir,
        "leader-1".to_string(),
        "".to_string(),
        admin_base_url.clone(),
    )
    .context("init cluster")?;
    let cluster_ca_pem = cluster
        .read_cluster_ca_pem(&leader_dir)
        .context("read cluster ca pem")?;
    let cluster_ca_key_pem = cluster
        .read_cluster_ca_key_pem(&leader_dir)
        .context("read cluster ca key pem")?;

    let leader_store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(store_init(
            &leader_dir,
            Some(cluster.node_id.clone()),
            "leader-1",
            &admin_base_url,
        ))
        .context("init leader store")?,
    ));
    let follower_store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(store_init(
            &follower_dir,
            Some(new_ulid_string()),
            "follower-1",
            "http://127.0.0.1:0",
        ))
        .context("init follower store")?,
    ));

    let cluster_name = "forwarding-raft-facade".to_string();
    let leader_id: NodeId = 1;
    let follower_id: NodeId = 2;

    let leader = start_raft(
        &leader_dir,
        cluster_name.clone(),
        leader_id,
        leader_store.clone(),
        ReconcileHandle::noop(),
        HttpNetworkFactory::new(),
    )
    .await
    .context("start leader raft")?;

    let follower = start_raft(
        &follower_dir,
        cluster_name,
        follower_id,
        follower_store.clone(),
        ReconcileHandle::noop(),
        HttpNetworkFactory::new(),
    )
    .await
    .context("start follower raft")?;

    let leader_rpc = spawn_raft_rpc_server(leader.raft())
        .await
        .context("spawn leader rpc")?;
    let follower_rpc = spawn_raft_rpc_server(follower.raft())
        .await
        .context("spawn follower rpc")?;

    let leader_meta = NodeMeta {
        name: "leader-1".to_string(),
        api_base_url: admin_base_url.clone(),
        raft_endpoint: leader_rpc.base_url.clone(),
    };
    let follower_meta = NodeMeta {
        name: "follower-1".to_string(),
        api_base_url: "".to_string(),
        raft_endpoint: follower_rpc.base_url.clone(),
    };

    leader
        .initialize_single_node_if_needed(leader_id, leader_meta.clone())
        .await
        .context("init leader")?;
    wait_for_leader(leader.metrics(), leader_id, Duration::from_secs(10)).await?;

    leader
        .add_learner(follower_id, follower_meta)
        .await
        .context("add follower learner")?;

    let leader_base_url =
        wait_for_leader_base_url(follower.metrics(), leader_id, Duration::from_secs(10)).await?;
    assert_eq!(leader_base_url, admin_base_url);

    let config = Config {
        bind: admin_addr,
        xray_api_addr: SocketAddr::from(([127, 0, 0, 1], 10085)),
        xray_health_interval_secs: 2,
        xray_health_fails_before_down: 3,
        xray_restart_mode: xp::config::XrayRestartMode::None,
        xray_restart_cooldown_secs: 30,
        xray_restart_timeout_secs: 5,
        xray_systemd_unit: "xray.service".to_string(),
        xray_openrc_service: "xray".to_string(),
        data_dir: leader_dir.clone(),
        admin_token_hash: test_admin_token_hash(&admin_token),
        node_name: cluster.node_name.clone(),
        access_host: cluster.access_host.clone(),
        api_base_url: admin_base_url.clone(),
        quota_poll_interval_secs: 10,
        quota_auto_unban: true,
    };

    let xray_health = xp::xray_supervisor::XrayHealthHandle::new_unknown();
    let raft_facade: Arc<dyn xp::raft::app::RaftFacade> = Arc::new(leader.clone());
    let endpoint_probe = xp::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        leader_store.clone(),
        raft_facade.clone(),
    );
    let router = build_router(
        config,
        leader_store.clone(),
        ReconcileHandle::noop(),
        xray_health,
        endpoint_probe,
        cluster,
        cluster_ca_pem.clone(),
        cluster_ca_key_pem.clone(),
        raft_facade,
        None,
    );

    let admin_server = spawn_server(admin_listener, router)
        .await
        .context("spawn admin server")?;

    let forwarding = ForwardingRaftFacade::try_new(
        follower.raft(),
        cluster_ca_key_pem
            .clone()
            .ok_or_else(|| anyhow::anyhow!("missing cluster ca key"))?,
        &cluster_ca_pem,
        None,
        None,
    )
    .context("build forwarding facade")?;

    let user = User {
        user_id: "user-forward".to_string(),
        display_name: "forwarded-write".to_string(),
        subscription_token: "sub_test_token".to_string(),
        quota_reset: UserQuotaReset::Monthly {
            day_of_month: 1,
            tz_offset_minutes: 480,
        },
    };
    let cmd = DesiredStateCommand::UpsertUser { user: user.clone() };

    let err = follower
        .raft()
        .client_write(cmd.clone())
        .await
        .expect_err("expected follower client_write to forward");
    let Some(openraft::error::ClientWriteError::ForwardToLeader(_)) = err.api_error() else {
        anyhow::bail!("expected ForwardToLeader error from follower, got {err}");
    };

    let resp = forwarding
        .client_write(cmd)
        .await
        .context("forwarding client_write")?;
    match resp {
        ClientResponse::Ok { .. } => {}
        other => anyhow::bail!("unexpected client_write response: {other:?}"),
    }

    let replicated = wait_for_user(&leader_store, &user.user_id, Duration::from_secs(10))
        .await
        .context("wait for user in leader store")?;
    assert_eq!(replicated, user);

    admin_server.shutdown().await?;
    leader_rpc.shutdown().await?;
    follower_rpc.shutdown().await?;

    Ok(())
}
