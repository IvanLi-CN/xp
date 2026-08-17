use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use clap::Parser as _;
use tokio::sync::{Mutex, watch};

use super::*;
use crate::{
    domain::{Node, NodeQuotaReset},
    node_history::{NodeHistoryHandle, NodeHistorySnapshot},
    raft::{
        app::{BoxFuture, LocalRaft},
        types::{ClientResponse, NodeMeta},
    },
    reconcile::{ReconcileHandle, ReconcileRequest},
    state::{JsonSnapshotStore, StoreInit},
};

#[derive(Clone)]
struct RepairingRaft {
    inner: LocalRaft,
    metrics: watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
    metrics_tx: watch::Sender<openraft::RaftMetrics<NodeId, NodeMeta>>,
    retain_values: Arc<Mutex<Vec<bool>>>,
    fail_next_membership_change: Arc<AtomicBool>,
}

impl RaftFacade for RepairingRaft {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>> {
        self.metrics.clone()
    }

    fn client_write(
        &self,
        command: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        self.inner.client_write(command)
    }

    fn add_learner(&self, _node_id: NodeId, _node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async { anyhow::bail!("unexpected add_learner") })
    }

    fn wait_learner_caught_up(
        &self,
        _node_id: NodeId,
        _required_log_index: u64,
        _timeout: Duration,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async { anyhow::bail!("unexpected learner catch-up") })
    }

    fn add_voters(&self, _node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async { anyhow::bail!("unexpected add_voters") })
    }

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<NodeId, NodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        let metrics = self.metrics.clone();
        let metrics_tx = self.metrics_tx.clone();
        let retain_values = self.retain_values.clone();
        let fail_next_membership_change = self.fail_next_membership_change.clone();
        Box::pin(async move {
            retain_values.lock().await.push(retain);
            if fail_next_membership_change.swap(false, Ordering::SeqCst) {
                anyhow::bail!("injected membership change failure")
            }
            let openraft::ChangeMembers::RemoveVoters(removed) = changes else {
                anyhow::bail!("unexpected membership change")
            };
            let mut next = metrics.borrow().clone();
            let previous = next.membership_config.clone();
            let voters = previous
                .membership()
                .voter_ids()
                .filter(|node_id| !removed.contains(node_id))
                .collect::<BTreeSet<_>>();
            let nodes = previous
                .nodes()
                .filter(|(node_id, _)| !removed.contains(node_id))
                .map(|(node_id, node)| (*node_id, node.clone()))
                .collect::<BTreeMap<_, _>>();
            next.membership_config = Arc::new(openraft::StoredMembership::new(
                previous.log_id().clone(),
                openraft::Membership::new(vec![voters], nodes),
            ));
            metrics_tx.send_replace(next);
            Ok(())
        })
    }
}

#[tokio::test]
async fn orphan_voter_repair_resumes_after_an_uncertain_remove_voters_request() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: None,
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::label_empty().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    let desired_node_id = xp_test_fixtures::identifier_ulid_d().to_owned();
    let local_node_id = raft_node_id_from_ulid(&desired_node_id).unwrap();
    let orphan_node_id = local_node_id.saturating_add(1);
    store
        .lock()
        .await
        .upsert_node(Node {
            node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            access_host: xp_test_fixtures::primary_host().to_owned(),
            api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();
    let desired_nodes_before = store.lock().await.list_nodes();

    let mut metrics = openraft::RaftMetrics::new_initial(local_node_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_node_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![BTreeSet::from([local_node_id, orphan_node_id])],
            BTreeMap::from([
                (
                    local_node_id,
                    NodeMeta {
                        name: xp_test_fixtures::primary_node_name().to_owned(),
                        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                    },
                ),
                (
                    orphan_node_id,
                    NodeMeta {
                        name: xp_test_fixtures::secondary_node_name().to_owned(),
                        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
                    },
                ),
            ]),
        ),
    ));
    let (metrics_tx, metrics_rx) = watch::channel(metrics);
    let retain_values = Arc::new(Mutex::new(Vec::new()));
    let fail_next_membership_change = Arc::new(AtomicBool::new(false));
    let raft: Arc<dyn RaftFacade> = Arc::new(RepairingRaft {
        inner: LocalRaft::new(store.clone(), metrics_rx.clone()),
        metrics: metrics_rx,
        metrics_tx,
        retain_values: retain_values.clone(),
        fail_next_membership_change: fail_next_membership_change.clone(),
    });

    let preview = preview_orphan_voter_repair(raft.clone(), store.clone(), orphan_node_id)
        .await
        .unwrap();
    assert!(store.lock().await.state().membership_operations.is_empty());

    fail_next_membership_change.store(true, Ordering::SeqCst);
    let error = repair_orphan_voter(
        raft.clone(),
        store.clone(),
        orphan_node_id,
        &preview.expected_membership,
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("injected membership change failure")
    );
    assert_eq!(
        store
            .lock()
            .await
            .state()
            .active_membership_operation()
            .unwrap()
            .phase,
        MembershipOperationPhase::Prepared
    );

    resume_membership_operations_once(raft.clone(), store.clone())
        .await
        .unwrap();

    assert_eq!(
        store
            .lock()
            .await
            .state()
            .membership_operations
            .values()
            .next()
            .unwrap()
            .phase,
        MembershipOperationPhase::Completed
    );
    assert_eq!(*retain_values.lock().await, vec![false, false]);
    assert!(
        raft.metrics()
            .borrow()
            .membership_config
            .membership()
            .get_node(&orphan_node_id)
            .is_none()
    );
    assert_eq!(store.lock().await.list_nodes(), desired_nodes_before);
}

#[tokio::test]
async fn remove_node_cleanup_stays_recoverable_until_runtime_and_history_cleanup_are_queued() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: None,
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::label_empty().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    let local_raft_node_id = 1;
    let target_node_id = xp_test_fixtures::identifier_ulid_a().to_owned();
    let destination_node = Node {
        node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
        node_name: xp_test_fixtures::secondary_node_name().to_owned(),
        access_host: xp_test_fixtures::secondary_host().to_owned(),
        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    store
        .lock()
        .await
        .upsert_node(destination_node.clone())
        .unwrap();

    let mut metrics = openraft::RaftMetrics::new_initial(local_raft_node_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_raft_node_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![BTreeSet::from([local_raft_node_id])],
            BTreeMap::from([(
                local_raft_node_id,
                NodeMeta {
                    name: xp_test_fixtures::primary_node_name().to_owned(),
                    api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                    raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                },
            )]),
        ),
    ));
    let expected_membership = membership_revision(&metrics).unwrap();
    let (_metrics_tx, metrics_rx) = watch::channel(metrics);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));
    store.lock().await.state_mut().membership_operations.insert(
        "remove-node".to_string(),
        MembershipOperation {
            operation_id: "remove-node".to_string(),
            kind: MembershipOperationKind::RemoveNode,
            raft_node_id: 2,
            node_id: Some(target_node_id.clone()),
            expected_membership,
            phase: MembershipOperationPhase::MembershipRemoved,
            legacy: false,
            delete_endpoints: true,
            expected_endpoint_ids: vec!["endpoint-target".to_string()],
            expected_endpoint_tags: vec!["inbound-target".to_string()],
            created_at: xp_test_fixtures::baseline_timestamp().to_owned(),
            next_retry_at: None,
            terminal_at: None,
            evidence: Some("desired node deleted".to_string()),
        },
    );

    let (reconcile_tx, mut reconcile_rx) = tokio::sync::mpsc::unbounded_channel();
    let data_dir = temp.path().to_string_lossy().into_owned();
    let config = crate::config::Cli::try_parse_from(["xp", "--data-dir", &data_dir])
        .unwrap()
        .config;
    let node_history = NodeHistoryHandle::from_config(&config);
    node_history
        .replace_node_snapshot(
            chrono::Utc::now(),
            &target_node_id,
            NodeHistorySnapshot {
                node_id: target_node_id.clone(),
                last_synced_at: None,
                last_sync_error: None,
                daily_traffic: Vec::new(),
                daily_component_status: Vec::new(),
                component_status_events: Vec::new(),
                traffic: None,
                user_traffic_users: Vec::new(),
            },
        )
        .await;
    let cleanup = MembershipRemovalCleanup {
        local_raft_node_id,
        local_node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
        node_history: node_history.clone(),
        reconcile: ReconcileHandle::from_sender(reconcile_tx),
    };

    let mut finalized = false;
    for _ in 0..100 {
        if finalize_remove_node_cleanup_once(raft.clone(), store.clone(), &cleanup)
            .await
            .unwrap()
        {
            finalized = true;
            break;
        }
        // Cleanup deliberately yields when another lifecycle step owns the shared gate.
        tokio::task::yield_now().await;
    }
    assert!(finalized);

    assert_eq!(
        store
            .lock()
            .await
            .state()
            .membership_operations
            .get("remove-node")
            .unwrap()
            .phase,
        MembershipOperationPhase::Completed
    );
    assert!(node_history.snapshot(&target_node_id).await.is_none());
    let mut requests = Vec::new();
    while let Ok(request) = reconcile_rx.try_recv() {
        requests.push(request);
    }
    assert!(requests.contains(&ReconcileRequest::RemoveInbound {
        tag: "inbound-target".to_string(),
    }));
    assert!(requests.contains(&ReconcileRequest::Full));
}

#[tokio::test]
async fn remove_node_recovery_blocks_a_legacy_stale_endpoint_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: None,
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::label_empty().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    let local_raft_node_id = 1;
    let target_node_id = xp_test_fixtures::identifier_ulid_a().to_owned();
    store
        .lock()
        .await
        .upsert_node(Node {
            node_id: target_node_id.clone(),
            node_name: xp_test_fixtures::secondary_node_name().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();
    store
        .lock()
        .await
        .create_endpoint(
            target_node_id.clone(),
            crate::domain::EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            8_388,
            serde_json::json!({}),
        )
        .unwrap();

    let mut metrics = openraft::RaftMetrics::new_initial(local_raft_node_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_raft_node_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![BTreeSet::from([local_raft_node_id])],
            BTreeMap::from([(
                local_raft_node_id,
                NodeMeta {
                    name: xp_test_fixtures::primary_node_name().to_owned(),
                    api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                    raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                },
            )]),
        ),
    ));
    let expected_membership = membership_revision(&metrics).unwrap();
    let (_metrics_tx, metrics_rx) = watch::channel(metrics);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));
    store.lock().await.state_mut().membership_operations.insert(
        "remove-node".to_string(),
        MembershipOperation {
            operation_id: "remove-node".to_string(),
            kind: MembershipOperationKind::RemoveNode,
            raft_node_id: 2,
            node_id: Some(target_node_id),
            expected_membership,
            phase: MembershipOperationPhase::MembershipRemoved,
            legacy: false,
            delete_endpoints: true,
            expected_endpoint_ids: vec!["stale-endpoint".to_string()],
            expected_endpoint_tags: vec!["stale-inbound".to_string()],
            created_at: xp_test_fixtures::baseline_timestamp().to_owned(),
            next_retry_at: None,
            terminal_at: None,
            evidence: Some("legacy stale snapshot".to_string()),
        },
    );

    for _ in 0..100 {
        resume_membership_operations_once(raft.clone(), store.clone())
            .await
            .unwrap();
        if store
            .lock()
            .await
            .state()
            .membership_operations
            .get("remove-node")
            .is_some_and(|operation| operation.phase == MembershipOperationPhase::Blocked)
        {
            break;
        }
        // Resume deliberately skips a busy shared lifecycle gate; periodic work retries it.
        tokio::task::yield_now().await;
    }

    let operation = store
        .lock()
        .await
        .state()
        .membership_operations
        .get("remove-node")
        .cloned()
        .unwrap();
    assert_eq!(operation.phase, MembershipOperationPhase::Blocked);
    assert!(
        operation
            .evidence
            .as_deref()
            .unwrap()
            .contains("node endpoint set changed")
    );
}
