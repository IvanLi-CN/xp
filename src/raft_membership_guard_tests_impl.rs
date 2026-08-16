use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::sync::{Mutex, watch};

use super::*;
use crate::{
    domain::{Node, NodeQuotaReset},
    raft::{
        app::{BoxFuture, LocalRaft},
        types::{ClientResponse, NodeMeta},
    },
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
