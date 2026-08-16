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

fn node() -> Node {
    Node {
        node_id: xp_test_fixtures::identifier_ulid_c().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        access_host: xp_test_fixtures::primary_host().to_owned(),
        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    }
}

fn meta() -> NodeMeta {
    NodeMeta {
        name: xp_test_fixtures::primary_node_name().to_owned(),
        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
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
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: "".to_string(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
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
        node_id: Some(xp_test_fixtures::identifier_ulid_c().to_owned()),
        expected_membership: "revision".to_string(),
        phase: MembershipOperationPhase::Prepared,
        legacy: false,
        delete_endpoints: false,
        expected_endpoint_ids: Vec::new(),
        created_at: xp_test_fixtures::baseline_timestamp().to_owned(),
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
    let voter_id =
        crate::raft::types::raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_c()).unwrap();
    let (_temp, raft, store) = context(
        BTreeSet::from([voter_id]),
        BTreeMap::from([(voter_id, meta())]),
    )
    .await;
    let mut store_guard = store.lock().await;
    for node_key in [
        xp_test_fixtures::identifier_ulid_c().to_owned(),
        xp_test_fixtures::identifier_ulid_c().to_ascii_lowercase(),
    ] {
        store_guard
            .state_mut()
            .nodes
            .insert(node_key.to_string(), node());
    }
    drop(store_guard);
    let audit = audit_membership(raft, store).await;
    assert_eq!(audit.duplicate_desired_members, BTreeSet::from([voter_id]));
    assert!(!audit.is_clean());
}
