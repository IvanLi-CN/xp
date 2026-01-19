use std::{collections::BTreeSet, path::Path, sync::Arc};

use anyhow::Context as _;
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot},
    task::JoinHandle,
    time::{Duration, Instant},
};

use xp::{
    domain::{CyclePolicyDefault, User},
    raft::storage::StorePaths,
    raft::{
        NodeId, NodeMeta,
        app::RaftFacade as _,
        http_rpc::{RaftRpcState, build_raft_rpc_router},
        network_http::HttpNetworkFactory,
        runtime::start_raft,
        types::TypeConfig,
    },
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
};

struct RpcServerHandle {
    base_url: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: JoinHandle<anyhow::Result<()>>,
}

impl RpcServerHandle {
    async fn shutdown(mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.join
            .await
            .context("join raft rpc server task")?
            .context("raft rpc server exited with error")?;
        Ok(())
    }
}

async fn spawn_raft_rpc_server(
    raft: openraft::Raft<TypeConfig>,
) -> anyhow::Result<RpcServerHandle> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("bind raft rpc listener")?;
    let addr = listener.local_addr().context("raft rpc local_addr")?;
    let base_url = format!("http://{addr}");

    let router = build_raft_rpc_router(RaftRpcState { raft });

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

    Ok(RpcServerHandle {
        base_url,
        shutdown_tx: Some(shutdown_tx),
        join,
    })
}

fn store_init(data_dir: &Path, bootstrap_node_id: String, node_name: String) -> StoreInit {
    StoreInit {
        data_dir: data_dir.to_path_buf(),
        bootstrap_node_id: Some(bootstrap_node_id),
        bootstrap_node_name: node_name,
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: "https://127.0.0.1:62416".to_string(),
    }
}

async fn wait_for_leader(
    mut rx: tokio::sync::watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
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

async fn wait_for_voter(
    mut rx: tokio::sync::watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
    voter_id: NodeId,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let m = rx.borrow();
            if m.membership_config.voter_ids().any(|id| id == voter_id) {
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            let m = rx.borrow();
            anyhow::bail!(
                "timeout waiting for voter_id={voter_id}; membership={}",
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
            anyhow::bail!("timeout waiting for replicated user_id={user_id}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_for_snapshot(
    raft: &openraft::Raft<TypeConfig>,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match raft.get_snapshot().await {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(e) => {
                // `get_snapshot()` may transiently error while a freshly-triggered snapshot is
                // being materialized on disk (e.g. metadata points to a snapshot file not yet
                // present). Treat "NotFound" as retriable within the timeout window.
                if !error_chain_has_not_found(&e) {
                    return Err(anyhow::anyhow!("raft get_snapshot: {e}"));
                }
            }
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timeout waiting for snapshot to be built");
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn error_chain_has_not_found(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current: &(dyn std::error::Error + 'static) = err;
    loop {
        if let Some(io) = current.downcast_ref::<std::io::Error>() {
            if io.kind() == std::io::ErrorKind::NotFound {
                return true;
            }
        }
        match current.source() {
            Some(next) => current = next,
            None => return false,
        }
    }
}

#[tokio::test]
async fn raft_two_node_replication_smoke() -> anyhow::Result<()> {
    run_raft_cluster_replication_smoke(2).await
}

#[tokio::test]
async fn raft_single_node_replication_smoke() -> anyhow::Result<()> {
    run_raft_cluster_replication_smoke(1).await
}

#[tokio::test]
async fn raft_three_node_replication_smoke() -> anyhow::Result<()> {
    run_raft_cluster_replication_smoke(3).await
}

#[tokio::test]
async fn raft_four_node_replication_smoke() -> anyhow::Result<()> {
    run_raft_cluster_replication_smoke(4).await
}

async fn run_raft_cluster_replication_smoke(node_count: usize) -> anyhow::Result<()> {
    anyhow::ensure!(node_count >= 1, "node_count must be >= 1");

    let tmp = tempfile::tempdir().context("tempdir")?;
    let mut node_dirs = Vec::with_capacity(node_count);
    for i in 1..=node_count {
        let dir = tmp.path().join(format!("node-{i}"));
        std::fs::create_dir_all(&dir).with_context(|| format!("create node-{i} dir"))?;
        node_dirs.push(dir);
    }

    let mut stores = Vec::with_capacity(node_count);
    for i in 1..=node_count {
        let dir = &node_dirs[i - 1];
        let store = JsonSnapshotStore::load_or_init(store_init(
            dir,
            xp::id::new_ulid_string(),
            format!("node-{i}"),
        ))
        .with_context(|| format!("init store-{i}"))?;
        stores.push(Arc::new(Mutex::new(store)));
    }

    let cluster_name = format!("raft-{node_count}-node-replication-smoke");

    let mut rafts = Vec::with_capacity(node_count);
    for i in 1..=node_count {
        let raft = start_raft(
            &node_dirs[i - 1],
            cluster_name.clone(),
            i as NodeId,
            stores[i - 1].clone(),
            ReconcileHandle::noop(),
            HttpNetworkFactory::new(),
        )
        .await
        .with_context(|| format!("start raft-{i}"))?;
        rafts.push(raft);
    }

    let mut rpcs = Vec::with_capacity(node_count);
    for i in 1..=node_count {
        let rpc = spawn_raft_rpc_server(rafts[i - 1].raft())
            .await
            .with_context(|| format!("rpc-{i}"))?;
        rpcs.push(rpc);
    }

    let mut metas = Vec::with_capacity(node_count);
    for i in 1..=node_count {
        metas.push(NodeMeta {
            name: format!("node-{i}"),
            api_base_url: "https://127.0.0.1:62416".to_string(),
            raft_endpoint: rpcs[i - 1].base_url.clone(),
        });
    }

    let leader_id: NodeId = 1;
    let leader = &rafts[0];
    leader
        .initialize_single_node_if_needed(leader_id, metas[0].clone())
        .await
        .context("initialize node-1")?;

    wait_for_leader(leader.metrics(), leader_id, Duration::from_secs(10)).await?;

    for i in 2..=node_count {
        leader
            .add_learner(i as NodeId, metas[i - 1].clone())
            .await
            .with_context(|| format!("add node-{i} learner"))?;
    }

    let user = User {
        user_id: "user-1".to_string(),
        display_name: "replication-smoke".to_string(),
        subscription_token: "sub_test_token".to_string(),
        cycle_policy_default: CyclePolicyDefault::ByUser,
        cycle_day_of_month_default: 1,
    };
    leader
        .client_write(DesiredStateCommand::UpsertUser { user: user.clone() })
        .await
        .context("client_write on leader")?;

    for i in 1..=node_count {
        let replicated = wait_for_user(&stores[i - 1], &user.user_id, Duration::from_secs(10))
            .await
            .with_context(|| format!("wait for replicated user on node-{i}"))?;
        assert_eq!(replicated, user);
    }

    if node_count > 1 {
        let voters = (2..=node_count)
            .map(|i| i as NodeId)
            .collect::<BTreeSet<_>>();
        leader
            .add_voters(voters.clone())
            .await
            .context("promote learners to voters")?;

        for node_id in voters {
            wait_for_voter(leader.metrics(), node_id, Duration::from_secs(15)).await?;
            let m = leader.metrics().borrow().clone();
            assert!(m.membership_config.voter_ids().any(|id| id == node_id));
            assert!(
                !m.membership_config
                    .membership()
                    .learner_ids()
                    .any(|id| id == node_id)
            );
        }
    }

    for rpc in rpcs {
        rpc.shutdown().await?;
    }

    Ok(())
}

#[tokio::test]
async fn raft_single_node_restart_recovers_state_and_snapshot_files() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir().context("tempdir")?;
    let node_dir = tmp.path().join("node-1");
    std::fs::create_dir_all(&node_dir).context("create node-1 dir")?;

    let bootstrap_node_id = xp::id::new_ulid_string();
    let cluster_name = "raft-single-node-restart-smoke".to_string();
    let node_id: NodeId = 1;

    {
        let store = Arc::new(Mutex::new(
            JsonSnapshotStore::load_or_init(store_init(
                &node_dir,
                bootstrap_node_id.clone(),
                "node-1".to_string(),
            ))
            .context("init store-1")?,
        ));

        let raft = start_raft(
            &node_dir,
            cluster_name.clone(),
            node_id,
            store.clone(),
            ReconcileHandle::noop(),
            HttpNetworkFactory::new(),
        )
        .await
        .context("start raft-1")?;

        let rpc = spawn_raft_rpc_server(raft.raft()).await.context("rpc-1")?;
        let meta = NodeMeta {
            name: "node-1".to_string(),
            api_base_url: "https://127.0.0.1:62416".to_string(),
            raft_endpoint: rpc.base_url.clone(),
        };

        raft.initialize_single_node_if_needed(node_id, meta)
            .await
            .context("initialize raft")?;
        wait_for_leader(raft.metrics(), node_id, Duration::from_secs(10)).await?;

        let user = User {
            user_id: "user-restart".to_string(),
            display_name: "restart-smoke".to_string(),
            subscription_token: "sub_test_token".to_string(),
            cycle_policy_default: CyclePolicyDefault::ByUser,
            cycle_day_of_month_default: 1,
        };
        raft.client_write(DesiredStateCommand::UpsertUser { user: user.clone() })
            .await
            .context("client_write")?;
        let got = wait_for_user(&store, &user.user_id, Duration::from_secs(10))
            .await
            .context("wait for user on leader")?;
        assert_eq!(got, user);

        let raft_handle = raft.raft();
        raft_handle
            .trigger()
            .snapshot()
            .await
            .map_err(|e| anyhow::anyhow!("trigger snapshot: {e}"))?;
        wait_for_snapshot(&raft_handle, Duration::from_secs(10)).await?;

        let paths = StorePaths::new(&node_dir);
        let meta_bytes =
            std::fs::read(&paths.snapshot_meta_json).context("read snapshot_meta_json")?;
        let snap_bytes =
            std::fs::read(&paths.snapshot_data_json).context("read snapshot_data_json")?;
        assert!(!meta_bytes.is_empty(), "snapshot meta must not be empty");
        assert!(!snap_bytes.is_empty(), "snapshot data must not be empty");

        rpc.shutdown().await?;
    }

    // Restart: reload store, start raft again, and ensure state is still present.
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(store_init(
            &node_dir,
            bootstrap_node_id,
            "node-1".to_string(),
        ))
        .context("reload store-1")?,
    ));
    let raft = start_raft(
        &node_dir,
        cluster_name,
        node_id,
        store.clone(),
        ReconcileHandle::noop(),
        HttpNetworkFactory::new(),
    )
    .await
    .context("restart raft-1")?;
    let rpc = spawn_raft_rpc_server(raft.raft())
        .await
        .context("restart rpc-1")?;
    let meta = NodeMeta {
        name: "node-1".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        raft_endpoint: rpc.base_url.clone(),
    };
    raft.initialize_single_node_if_needed(node_id, meta)
        .await
        .context("initialize after restart")?;
    wait_for_leader(raft.metrics(), node_id, Duration::from_secs(10)).await?;

    {
        let store_guard = store.lock().await;
        let user = store_guard
            .get_user("user-restart")
            .expect("expected user to persist after restart");
        assert_eq!(user.display_name, "restart-smoke");
    }

    rpc.shutdown().await?;
    Ok(())
}
