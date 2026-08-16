use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use tokio::sync::{Mutex, watch};

use crate::{
    domain::{Node, NodeQuotaReset},
    raft::{
        app::{LocalRaft, RaftFacade},
        types::{NodeId, NodeMeta},
    },
    raft_membership_guard::audit_membership,
    state::{
        JsonSnapshotStore, MembershipOperation, MembershipOperationKind, MembershipOperationPhase,
        StoreInit,
    },
};

fn desired_node_id(raft_node_id: NodeId, variant: u64) -> String {
    ulid::Ulid::from(((variant as u128) << 64) | raft_node_id as u128).to_string()
}

fn node(node_id: String) -> Node {
    Node {
        node_id: node_id.clone(),
        node_name: node_id,
        access_host: "node.example".to_string(),
        api_base_url: "https://node.example".to_string(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    }
}

fn meta() -> NodeMeta {
    NodeMeta {
        name: "node".to_string(),
        api_base_url: "https://node.example".to_string(),
        raft_endpoint: "https://node.example".to_string(),
    }
}

async fn context(
    voters: BTreeSet<NodeId>,
    members: BTreeMap<NodeId, NodeMeta>,
) -> (
    tempfile::TempDir,
    Arc<dyn RaftFacade>,
    Arc<Mutex<JsonSnapshotStore>>,
) {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: None,
            bootstrap_node_name: "node".to_string(),
            bootstrap_access_host: "".to_string(),
            bootstrap_api_base_url: "https://node.example".to_string(),
        })
        .unwrap(),
    ));
    let mut metrics = openraft::RaftMetrics::new_initial(1);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(vec![voters], members),
    ));
    let (_, metrics) = watch::channel(metrics);
    (
        temp,
        Arc::new(LocalRaft::new(store.clone(), metrics)),
        store,
    )
}

fn operation(kind: MembershipOperationKind, raft_node_id: NodeId) -> MembershipOperation {
    MembershipOperation {
        operation_id: "operation".to_string(),
        kind,
        raft_node_id,
        node_id: None,
        expected_membership: "revision".to_string(),
        phase: MembershipOperationPhase::Prepared,
        legacy: false,
        delete_endpoints: false,
        expected_endpoint_ids: Vec::new(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        next_retry_at: None,
        terminal_at: None,
        evidence: None,
    }
}

#[tokio::test]
async fn audit_allows_only_the_precise_join_or_restore_learner_shape() {
    let learner_id = 42;
    let (_temp, raft, store) =
        context(BTreeSet::new(), BTreeMap::from([(learner_id, meta())])).await;
    store.lock().await.state_mut().membership_operations.insert(
        "operation".to_string(),
        operation(MembershipOperationKind::Join, learner_id),
    );
    assert_eq!(
        audit_membership(raft.clone(), store.clone())
            .await
            .unexpected_learners,
        BTreeSet::from([learner_id])
    );

    store.lock().await.state_mut().membership_operations.insert(
        "operation".to_string(),
        operation(MembershipOperationKind::Restore, learner_id),
    );
    assert!(
        audit_membership(raft, store)
            .await
            .unexpected_learners
            .is_empty()
    );
}

#[tokio::test]
async fn audit_blocks_duplicate_desired_identity_mappings() {
    let voter_id = 42;
    let (_temp, raft, store) = context(
        BTreeSet::from([voter_id]),
        BTreeMap::from([(voter_id, meta())]),
    )
    .await;
    let mut store_guard = store.lock().await;
    for variant in [1, 2] {
        let node_id = desired_node_id(voter_id, variant);
        store_guard
            .state_mut()
            .nodes
            .insert(node_id.clone(), node(node_id));
    }
    drop(store_guard);
    let audit = audit_membership(raft, store).await;
    assert_eq!(audit.duplicate_desired_members, BTreeSet::from([voter_id]));
    assert!(!audit.is_clean());
}
