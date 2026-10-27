use std::sync::Arc;

use tokio::sync::{Mutex, watch};

use crate::{
    join_coordinator::migrate_one_legacy_join_session,
    join_session::{JoinSession, JoinSessionStatus},
    raft::{
        app::{LocalRaft, RaftFacade},
        types::raft_node_id_from_ulid,
    },
    state::{JsonSnapshotStore, StoreInit},
};

#[tokio::test]
async fn non_replayable_legacy_session_is_tombstoned_after_blocking() {
    let tmp = tempfile::tempdir().unwrap();
    let stale_node_id = xp_test_fixtures::identifier_ulid_b().to_owned();
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
    store.lock().await.state_mut().join_sessions.insert(
        stale_node_id.clone(),
        JoinSession {
            node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
            request_fingerprint: xp_test_fixtures::token_fixture512().to_owned(),
            signed_cert_pem: xp_test_fixtures::token_fixture555().to_owned(),
            token_expires_at: (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339(),
            activation_deadline: (chrono::Utc::now() - chrono::Duration::minutes(10)).to_rfc3339(),
            required_log_index: 1,
            status: JoinSessionStatus::Reserved,
            terminal_at: None,
        },
    );

    let leader_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_a()).unwrap();
    let mut metrics = openraft::RaftMetrics::new_initial(leader_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(leader_id);
    let (_tx, rx) = watch::channel(metrics);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), rx));

    migrate_one_legacy_join_session(&raft, &store)
        .await
        .unwrap();

    let store = store.lock().await;
    assert_eq!(
        store.state().join_sessions[&stale_node_id].status,
        JoinSessionStatus::Expired
    );
    assert!(
        store.state().join_sessions[&stale_node_id]
            .terminal_at
            .is_some()
    );
    assert!(store.state().active_membership_operation().is_none());
    assert_eq!(store.state().membership_operations.len(), 1);
    assert_eq!(
        store
            .state()
            .membership_operations
            .values()
            .next()
            .unwrap()
            .phase,
        crate::state::MembershipOperationPhase::Blocked
    );
}
