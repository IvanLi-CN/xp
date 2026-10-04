use std::{collections::BTreeSet, path::Path, sync::Arc};

use anyhow::Context as _;
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot},
    task::JoinHandle,
    time::{Duration, Instant},
};

use xp::{
    cluster_identity::generate_cluster_ca,
    domain::{User, UserQuotaReset},
    internal_auth::{
        self, InternalRoute, RequestContext, VerifiedRequest, sign_request_v2, verify_ack_v2,
        verify_request_v2,
    },
    raft::storage::StorePaths,
    raft::{
        NodeId, NodeMeta,
        app::RaftFacade as _,
        http_rpc::{
            RaftRpcAuth, RaftRpcState, build_authenticated_raft_rpc_router, build_raft_rpc_router,
        },
        network_http::HttpNetworkFactory,
        runtime::start_raft,
        types::TypeConfig,
    },
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
};

static RAFT_REPLICATION_TEST_LOCK: Mutex<()> = Mutex::const_new(());

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
    spawn_raft_rpc_router(build_raft_rpc_router(RaftRpcState {
        raft,
        reconcile: ReconcileHandle::noop(),
    }))
    .await
}

async fn spawn_raft_rpc_router(router: axum::Router) -> anyhow::Result<RpcServerHandle> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("bind raft rpc listener")?;
    let addr = listener.local_addr().context("raft rpc local_addr")?;
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

    Ok(RpcServerHandle {
        base_url,
        shutdown_tx: Some(shutdown_tx),
        join,
    })
}

fn signed_snapshot_request(
    base_url: &str,
    body: &[u8],
    ca_key_pem: &str,
    ca_cert_pem: &str,
    cluster_id: &str,
    node_id: &str,
) -> anyhow::Result<(reqwest::RequestBuilder, VerifiedRequest)> {
    let method = axum::http::Method::POST;
    let uri: axum::http::Uri = format!("{base_url}/raft/snapshot").parse()?;
    let context = RequestContext::now(
        InternalRoute::MeshV2,
        cluster_id,
        node_id,
        node_id,
        xp::id::new_ulid_string(),
    );
    let mut headers = axum::http::HeaderMap::new();
    sign_request_v2(
        ca_key_pem,
        ca_cert_pem,
        &method,
        &uri,
        Some("application/json"),
        body,
        &context,
        &mut headers,
    )?;
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        "application/json".parse()?,
    );
    headers.insert(
        axum::http::header::CONTENT_LENGTH,
        body.len().to_string().parse()?,
    );
    let verified = verify_request_v2(
        ca_key_pem,
        ca_cert_pem,
        &method,
        &uri,
        &headers,
        body,
        cluster_id,
        node_id,
    )?;
    let request = reqwest::Client::new()
        .post(uri.to_string())
        .headers(headers)
        .body(body.to_vec());
    Ok((request, verified))
}

fn store_init(data_dir: &Path, bootstrap_node_id: String, node_name: String) -> StoreInit {
    StoreInit {
        data_dir: data_dir.to_path_buf(),
        bootstrap_node_id: Some(bootstrap_node_id),
        bootstrap_node_name: node_name,
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: xp_test_fixtures::subscription_api_loopback_https().to_owned(),
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
                // Snapshot payload and metadata are written separately, so a read can observe
                // their brief mismatch while a fresh snapshot is being materialized.
                if !error_chain_is_transient_snapshot_read(&e) {
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

async fn wait_for_snapshot_completion(
    raft: &openraft::Raft<TypeConfig>,
    expected: &Option<openraft::LogId<NodeId>>,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let metrics = raft.metrics().borrow().clone();
        if metrics.snapshot.as_ref() == expected.as_ref() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "timeout waiting for snapshot completion: expected={expected:?}, \
                 last_applied={:?}, snapshot={:?}",
                metrics.last_applied,
                metrics.snapshot
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_for_applied_after(
    raft: &openraft::Raft<TypeConfig>,
    previous: &Option<openraft::LogId<NodeId>>,
    timeout: Duration,
) -> anyhow::Result<Option<openraft::LogId<NodeId>>> {
    let deadline = Instant::now() + timeout;
    loop {
        let last_applied = raft.metrics().borrow().last_applied.clone();
        if last_applied.as_ref() > previous.as_ref() {
            return Ok(last_applied);
        }
        if Instant::now() >= deadline {
            anyhow::bail!("timeout waiting for Raft apply after {previous:?}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn error_chain_is_transient_snapshot_read(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current: &(dyn std::error::Error + 'static) = err;
    loop {
        if let Some(io) = current.downcast_ref::<std::io::Error>() {
            if io.kind() == std::io::ErrorKind::NotFound {
                return true;
            }
        }
        if current
            .to_string()
            .ends_with("snapshot payload metadata mismatch")
        {
            return true;
        }
        match current.source() {
            Some(next) => current = next,
            None => return false,
        }
    }
}

#[cfg(test)]
mod snapshot_read_tests {
    use super::error_chain_is_transient_snapshot_read;
    use std::fmt;

    #[derive(Debug)]
    struct SnapshotReadContext(std::io::Error);

    impl fmt::Display for SnapshotReadContext {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "when Read Snapshot(None): std::io::error::Error: {}",
                self.0
            )
        }
    }

    impl std::error::Error for SnapshotReadContext {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn retries_the_known_snapshot_materialization_error_through_storage_context() {
        let error =
            SnapshotReadContext(std::io::Error::other("snapshot payload metadata mismatch"));

        assert!(error_chain_is_transient_snapshot_read(&error));
    }

    #[test]
    fn does_not_retry_unrecognized_snapshot_storage_errors() {
        let error = std::io::Error::other("snapshot checksum mismatch");

        assert!(!error_chain_is_transient_snapshot_read(&error));
    }
}

#[tokio::test]
async fn raft_two_node_replication_smoke() -> anyhow::Result<()> {
    let _serial = RAFT_REPLICATION_TEST_LOCK.lock().await;
    run_raft_cluster_replication_smoke(2).await
}

#[tokio::test]
async fn raft_single_node_replication_smoke() -> anyhow::Result<()> {
    let _serial = RAFT_REPLICATION_TEST_LOCK.lock().await;
    run_raft_cluster_replication_smoke(1).await
}

#[tokio::test]
async fn raft_three_node_replication_smoke() -> anyhow::Result<()> {
    let _serial = RAFT_REPLICATION_TEST_LOCK.lock().await;
    run_raft_cluster_replication_smoke(3).await
}

#[tokio::test]
async fn raft_four_node_replication_smoke() -> anyhow::Result<()> {
    let _serial = RAFT_REPLICATION_TEST_LOCK.lock().await;
    run_raft_cluster_replication_smoke(4).await
}

#[tokio::test]
async fn signed_snapshot_admission_rejects_before_openraft_and_retries_successfully()
-> anyhow::Result<()> {
    let _serial = RAFT_REPLICATION_TEST_LOCK.lock().await;
    let tmp = tempfile::tempdir().context("tempdir")?;
    let source_dir = tmp.path().join("source");
    let target_dir = tmp.path().join("target");
    std::fs::create_dir_all(&source_dir).context("create source directory")?;
    std::fs::create_dir_all(&target_dir).context("create target directory")?;

    let cluster_id = xp_test_fixtures::primary_cluster_id();
    let ca = generate_cluster_ca(cluster_id).context("generate cluster CA")?;
    let source_identity = xp::id::new_ulid_string();
    let target_identity = xp::id::new_ulid_string();
    let source_store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(store_init(
            &source_dir,
            source_identity,
            "snapshot-source".to_owned(),
        ))
        .context("init source store")?,
    ));
    let target_store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(store_init(
            &target_dir,
            target_identity.clone(),
            "snapshot-target".to_owned(),
        ))
        .context("init target store")?,
    ));
    let cluster_name = "raft-snapshot-admission-smoke".to_owned();
    let source = start_raft(
        &source_dir,
        cluster_name.clone(),
        1,
        source_store,
        ReconcileHandle::noop(),
        HttpNetworkFactory::new(),
    )
    .await
    .context("start source Raft")?;
    let target_reconcile = ReconcileHandle::noop();
    let target = start_raft(
        &target_dir,
        cluster_name,
        2,
        target_store.clone(),
        target_reconcile.clone(),
        HttpNetworkFactory::new(),
    )
    .await
    .context("start target Raft")?;

    source
        .initialize_single_node_if_needed(
            1,
            NodeMeta {
                name: "snapshot-source".to_owned(),
                api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
                raft_endpoint: "http://127.0.0.1:1".to_owned(),
            },
        )
        .await
        .context("initialize source Raft")?;
    wait_for_leader(source.metrics(), 1, Duration::from_secs(10)).await?;
    let user = User {
        user_id: "snapshot-admission-user".to_owned(),
        display_name: "snapshot-admission".to_owned(),
        subscription_token: xp_test_fixtures::label_sub_test_token().to_owned(),
        credential_epoch: 0,
        priority_tier: Default::default(),
        quota_reset: UserQuotaReset::Monthly {
            day_of_month: 1,
            tz_offset_minutes: 480,
        },
    };
    source
        .client_write(DesiredStateCommand::UpsertUser { user: user.clone() })
        .await
        .context("write snapshot test state")?;
    source
        .raft()
        .trigger()
        .snapshot()
        .await
        .context("trigger source snapshot")?;
    wait_for_snapshot(&source.raft(), Duration::from_secs(10)).await?;
    let snapshot = source
        .raft()
        .get_snapshot()
        .await
        .context("read source snapshot")?
        .context("source snapshot missing")?;
    let snapshot_log_id = snapshot.meta.last_log_id.clone();
    wait_for_snapshot_completion(&source.raft(), &snapshot_log_id, Duration::from_secs(10)).await?;
    let snapshot_request = openraft::raft::InstallSnapshotRequest::<TypeConfig> {
        vote: openraft::Vote::new_committed(1, 1),
        meta: snapshot.meta,
        offset: 0,
        data: (*snapshot.snapshot).into_inner(),
        done: true,
    };
    let body = serde_json::to_vec(&snapshot_request).context("encode snapshot request")?;
    let router_state = RaftRpcState {
        raft: target.raft(),
        reconcile: target_reconcile.clone(),
    };
    let auth = RaftRpcAuth {
        cluster_id: cluster_id.to_owned(),
        local_node_id: target_identity.clone(),
        cluster_ca_key_pem: ca.key_pem.clone(),
        cluster_ca_cert_pem: ca.cert_pem.clone(),
        store: target_store.clone(),
        bootstrap_sender: None,
    };
    let rpc = spawn_raft_rpc_router(build_authenticated_raft_rpc_router(router_state, auth))
        .await
        .context("start authenticated snapshot RPC")?;

    let held_mesh_read = target_reconcile.mesh_gate_lock().read_owned().await;
    let (request, verified) = signed_snapshot_request(
        &rpc.base_url,
        &body,
        &ca.key_pem,
        &ca.cert_pem,
        cluster_id,
        &target_identity,
    )?;
    let response = request.send().await.context("send contended snapshot")?;
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    let ack = response
        .headers()
        .get(internal_auth::INTERNAL_ACK_HEADER)
        .context("signed rejection acknowledgement missing")?
        .to_str()
        .context("snapshot acknowledgement is not ASCII")?;
    verify_ack_v2(
        &ca.key_pem,
        &ca.cert_pem,
        &verified,
        &target_identity,
        reqwest::StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        ack,
    )
    .context("verify signed admission rejection")?;
    assert_ne!(
        target.metrics().borrow().state,
        openraft::ServerState::Shutdown,
        "admission contention must not shut down the OpenRaft worker"
    );
    assert_eq!(
        target.metrics().borrow().last_applied,
        None,
        "rejected snapshot must not be dispatched to OpenRaft"
    );

    drop(held_mesh_read);
    let (request, _) = signed_snapshot_request(
        &rpc.base_url,
        &body,
        &ca.key_pem,
        &ca.cert_pem,
        cluster_id,
        &target_identity,
    )?;
    let response = request.send().await.context("retry snapshot")?;
    anyhow::ensure!(
        response.status().is_success(),
        "snapshot retry was rejected"
    );
    wait_for_user(&target_store, &user.user_id, Duration::from_secs(10))
        .await
        .context("wait for snapshot state on target")?;
    assert_ne!(
        target.metrics().borrow().state,
        openraft::ServerState::Shutdown,
        "successful snapshot installation must leave Raft running"
    );
    assert_eq!(
        target.metrics().borrow().last_applied,
        snapshot_log_id,
        "target applied index should advance to the snapshot boundary"
    );

    let cancellation_user = User {
        user_id: "snapshot-cancel-user".to_owned(),
        display_name: "snapshot-cancel".to_owned(),
        ..user
    };
    source
        .client_write(DesiredStateCommand::UpsertUser {
            user: cancellation_user.clone(),
        })
        .await
        .context("write snapshot cancellation state")?;
    let cancellation_log_id =
        wait_for_applied_after(&source.raft(), &snapshot_log_id, Duration::from_secs(10))
            .await
            .context("wait for cancellation command to apply")?;
    source
        .raft()
        .trigger()
        .snapshot()
        .await
        .context("trigger cancellation snapshot")?;
    wait_for_snapshot_completion(
        &source.raft(),
        &cancellation_log_id,
        Duration::from_secs(30),
    )
    .await
    .context("wait for cancellation snapshot")?;
    let cancellation_snapshot = source
        .raft()
        .get_snapshot()
        .await
        .context("read cancellation snapshot")?
        .context("cancellation snapshot missing")?;
    let cancellation_request = openraft::raft::InstallSnapshotRequest::<TypeConfig> {
        vote: openraft::Vote::new_committed(1, 1),
        meta: cancellation_snapshot.meta,
        offset: 0,
        data: (*cancellation_snapshot.snapshot).into_inner(),
        done: true,
    };
    let cancellation_rpc = spawn_raft_rpc_router(build_raft_rpc_router(RaftRpcState {
        raft: target.raft(),
        reconcile: target_reconcile.clone(),
    }))
    .await
    .context("start cancellation snapshot RPC")?;
    let held_store = target_store.clone().lock_owned().await;
    let request_body = serde_json::to_vec(&cancellation_request)
        .context("encode cancellation snapshot request")?;
    let request_url = format!("{}/raft/snapshot", cancellation_rpc.base_url);
    let request_task = tokio::spawn(async move {
        reqwest::Client::new()
            .post(request_url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(request_body)
            .send()
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !target_reconcile
            .snapshot_installing()
            .load(std::sync::atomic::Ordering::Acquire)
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .context("snapshot request should reserve Mesh before OpenRaft install")?;
    request_task.abort();
    let _ = request_task.await;
    assert!(
        target_reconcile
            .snapshot_installing()
            .load(std::sync::atomic::Ordering::Acquire),
        "disconnecting the HTTP caller must not release an admitted installation"
    );
    assert!(
        target_reconcile
            .mesh_gate_read_until(std::time::Instant::now() + Duration::from_millis(20))
            .await
            .is_none(),
        "new Mesh work must remain blocked after the HTTP caller disconnects"
    );
    drop(held_store);
    let installed_user = wait_for_user(
        &target_store,
        &cancellation_user.user_id,
        Duration::from_secs(10),
    )
    .await
    .context("wait for cancelled-request snapshot installation")?;
    assert_eq!(installed_user, cancellation_user);
    tokio::time::timeout(Duration::from_secs(10), async {
        while target_reconcile
            .snapshot_installing()
            .load(std::sync::atomic::Ordering::Acquire)
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .context("snapshot task should release Mesh reservation after install")?;
    assert!(
        target_reconcile
            .mesh_gate_read_until(std::time::Instant::now() + Duration::from_secs(1))
            .await
            .is_some(),
        "Mesh admission should resume after the owned install task terminates"
    );

    rpc.shutdown().await?;
    cancellation_rpc.shutdown().await?;
    Ok(())
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
            api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
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
        subscription_token: xp_test_fixtures::label_sub_test_token().to_owned(),
        credential_epoch: 0,
        priority_tier: Default::default(),
        quota_reset: UserQuotaReset::Monthly {
            day_of_month: 1,
            tz_offset_minutes: 480,
        },
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
            api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
            raft_endpoint: rpc.base_url.clone(),
        };

        raft.initialize_single_node_if_needed(node_id, meta)
            .await
            .context("initialize raft")?;
        wait_for_leader(raft.metrics(), node_id, Duration::from_secs(10)).await?;

        let user = User {
            user_id: "user-restart".to_string(),
            display_name: "restart-smoke".to_string(),
            subscription_token: xp_test_fixtures::label_sub_test_token().to_owned(),
            credential_epoch: 0,
            priority_tier: Default::default(),
            quota_reset: UserQuotaReset::Monthly {
                day_of_month: 1,
                tz_offset_minutes: 480,
            },
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
        api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
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
