use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use chrono::Utc;
use tokio::sync::{Mutex, watch};

use super::reconcile_once;
use crate::{
    domain::{Node, NodeQuotaReset},
    join_session::{JoinSession, JoinSessionStatus},
    raft::{
        app::{BoxFuture, LocalRaft, RaftFacade},
        types::{ClientResponse, NodeMeta as RaftNodeMeta, raft_node_id_from_ulid},
    },
    state::{
        DesiredStateCommand, JsonSnapshotStore, MembershipOperation, MembershipOperationKind,
        MembershipOperationPhase, StoreInit,
    },
};

#[derive(Clone)]
struct PromotionRevalidationRaft {
    inner: LocalRaft,
    revalidation_required: Arc<AtomicBool>,
}

impl RaftFacade for PromotionRevalidationRaft {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<u64, RaftNodeMeta>> {
        self.inner.metrics()
    }

    fn ensure_linearizable(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        let inner = self.inner.clone();
        let revalidation_required = self.revalidation_required.clone();
        Box::pin(async move {
            inner.ensure_linearizable().await?;
            revalidation_required.store(false, Ordering::SeqCst);
            Ok(())
        })
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        self.inner.client_write(cmd)
    }

    fn add_learner(&self, _node_id: u64, _node: RaftNodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async { anyhow::bail!("unexpected add_learner") })
    }

    fn wait_learner_caught_up(
        &self,
        _node_id: u64,
        _required_log_index: u64,
        _timeout: Duration,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        let revalidation_required = self.revalidation_required.clone();
        Box::pin(async move {
            revalidation_required.store(true, Ordering::SeqCst);
            Ok(())
        })
    }

    fn add_voters(&self, _node_ids: BTreeSet<u64>) -> BoxFuture<'_, anyhow::Result<()>> {
        let revalidation_required = self.revalidation_required.clone();
        Box::pin(async move {
            if revalidation_required.load(Ordering::SeqCst) {
                anyhow::bail!("promotion requires a fresh linearizable membership check")
            }
            Ok(())
        })
    }

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<u64, RaftNodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        self.inner.change_membership(changes, retain)
    }
}

async fn insert_join_operation(
    store: &Arc<Mutex<JsonSnapshotStore>>,
    metrics: &openraft::RaftMetrics<u64, RaftNodeMeta>,
) {
    let raft_node_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_b()).unwrap();
    store.lock().await.state_mut().membership_operations.insert(
        "join-operation".to_string(),
        MembershipOperation {
            operation_id: "join-operation".to_string(),
            kind: MembershipOperationKind::Join,
            raft_node_id,
            node_id: Some(xp_test_fixtures::identifier_ulid_b().to_owned()),
            expected_membership: crate::raft_membership_guard::membership_revision(metrics)
                .unwrap(),
            phase: MembershipOperationPhase::LearnerRegistered,
            legacy: false,
            remove_learner: false,
            delete_endpoints: false,
            expected_endpoint_ids: Vec::new(),
            expected_endpoint_tags: Vec::new(),
            created_at: xp_test_fixtures::baseline_timestamp().to_owned(),
            next_retry_at: None,
            terminal_at: None,
            evidence: Some("test join operation".to_string()),
        },
    );
}

#[tokio::test]
async fn promotion_revalidates_membership_after_learner_catch_up() {
    let tmp = tempfile::tempdir().unwrap();
    let learner_id = xp_test_fixtures::identifier_ulid_b().to_owned();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: tmp.path().to_owned(),
            bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_a().to_owned()),
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    {
        let mut store = store.lock().await;
        store.state_mut().nodes.insert(
            learner_id.clone(),
            Node {
                node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
                node_name: xp_test_fixtures::secondary_node_name().to_owned(),
                access_host: xp_test_fixtures::secondary_host().to_owned(),
                api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            },
        );
        store.state_mut().join_sessions.insert(
            learner_id.clone(),
            JoinSession {
                node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
                request_fingerprint: xp_test_fixtures::token_fixture512().to_owned(),
                signed_cert_pem: xp_test_fixtures::token_fixture555().to_owned(),
                token_expires_at: (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                activation_deadline: (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339(),
                required_log_index: 1,
                status: JoinSessionStatus::LearnerRegistered,
                terminal_at: None,
            },
        );
    }
    let leader_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_a()).unwrap();
    let learner_raft_id = raft_node_id_from_ulid(&learner_id).unwrap();
    let mut metrics = openraft::RaftMetrics::new_initial(leader_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(leader_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![BTreeSet::from([leader_id])],
            std::collections::BTreeMap::from([
                (
                    leader_id,
                    RaftNodeMeta {
                        name: xp_test_fixtures::primary_node_name().to_owned(),
                        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                    },
                ),
                (
                    learner_raft_id,
                    RaftNodeMeta {
                        name: xp_test_fixtures::secondary_node_name().to_owned(),
                        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
                    },
                ),
            ]),
        ),
    ));
    insert_join_operation(&store, &metrics).await;
    let (_tx, rx) = watch::channel(metrics);
    let revalidation_required = Arc::new(AtomicBool::new(false));
    let raft: Arc<dyn RaftFacade> = Arc::new(PromotionRevalidationRaft {
        inner: LocalRaft::new(store.clone(), rx),
        revalidation_required: revalidation_required.clone(),
    });

    reconcile_once(raft, store.clone()).await.unwrap();

    assert!(!revalidation_required.load(Ordering::SeqCst));
    assert_eq!(
        store
            .lock()
            .await
            .state()
            .join_sessions
            .get(&learner_id)
            .unwrap()
            .status,
        JoinSessionStatus::Consumed
    );
}
