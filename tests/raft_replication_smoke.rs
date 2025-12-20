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

async fn spawn_raft_rpc_server(raft: openraft::Raft<TypeConfig>) -> anyhow::Result<RpcServerHandle> {
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

fn store_init(data_dir: &Path, bootstrap_node_id: &str, node_name: &str) -> StoreInit {
    StoreInit {
        data_dir: data_dir.to_path_buf(),
        bootstrap_node_id: Some(bootstrap_node_id.to_string()),
        bootstrap_node_name: node_name.to_string(),
        bootstrap_public_domain: "".to_string(),
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

#[tokio::test]
async fn raft_two_node_replication_smoke() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir().context("tempdir")?;
    let node1_dir = tmp.path().join("node-1");
    let node2_dir = tmp.path().join("node-2");
    std::fs::create_dir_all(&node1_dir).context("create node-1 dir")?;
    std::fs::create_dir_all(&node2_dir).context("create node-2 dir")?;

    let store1 = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(store_init(
            &node1_dir,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "node-1",
        ))
        .context("init store-1")?,
    ));
    let store2 = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(store_init(
            &node2_dir,
            "01ARZ3NDEKTSV4RRFFQ69G5FB0",
            "node-2",
        ))
        .context("init store-2")?,
    ));

    let cluster_name = "raft-two-node-replication-smoke".to_string();

    let node1_id: NodeId = 1;
    let node2_id: NodeId = 2;

    let raft1 = start_raft(
        &node1_dir,
        cluster_name.clone(),
        node1_id,
        store1.clone(),
        ReconcileHandle::noop(),
        HttpNetworkFactory::new(),
    )
    .await
    .context("start raft-1")?;
    let raft2 = start_raft(
        &node2_dir,
        cluster_name,
        node2_id,
        store2.clone(),
        ReconcileHandle::noop(),
        HttpNetworkFactory::new(),
    )
    .await
    .context("start raft-2")?;

    let rpc1 = spawn_raft_rpc_server(raft1.raft()).await.context("rpc-1")?;
    let rpc2 = spawn_raft_rpc_server(raft2.raft()).await.context("rpc-2")?;

    let node1_meta = NodeMeta {
        name: "node-1".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        raft_endpoint: rpc1.base_url.clone(),
    };
    let node2_meta = NodeMeta {
        name: "node-2".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        raft_endpoint: rpc2.base_url.clone(),
    };

    raft1
        .initialize_single_node_if_needed(node1_id, node1_meta.clone())
        .await
        .context("initialize node-1")?;

    wait_for_leader(raft1.metrics(), node1_id, Duration::from_secs(8)).await?;

    raft1
        .add_learner(node2_id, node2_meta)
        .await
        .context("add node-2 learner")?;

    let user = User {
        user_id: "user-1".to_string(),
        display_name: "replication-smoke".to_string(),
        subscription_token: "sub_test_token".to_string(),
        cycle_policy_default: CyclePolicyDefault::ByUser,
        cycle_day_of_month_default: 1,
    };
    raft1
        .client_write(DesiredStateCommand::UpsertUser { user: user.clone() })
        .await
        .context("client_write on leader")?;

    let replicated =
        wait_for_user(&store2, &user.user_id, Duration::from_secs(8)).await?;
    assert_eq!(replicated, user);

    raft1
        .add_voters(BTreeSet::from([node2_id]))
        .await
        .context("promote node-2 to voter")?;
    wait_for_voter(raft1.metrics(), node2_id, Duration::from_secs(8)).await?;
    let m = raft1.metrics().borrow().clone();
    assert!(m.membership_config.voter_ids().any(|id| id == node2_id));
    assert!(!m
        .membership_config
        .membership()
        .learner_ids()
        .any(|id| id == node2_id));

    rpc1.shutdown().await?;
    rpc2.shutdown().await?;

    Ok(())
}
