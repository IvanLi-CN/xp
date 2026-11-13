use super::*;
use crate::http::history_repository::derived_repository_identity;
use crate::state::history_repository::control::{
    RepositoryCapacity, RepositoryMember, RepositoryMembership,
};
#[path = "../backfill/tests/app_state.rs"]
mod fixture;

#[tokio::test]
async fn former_repository_keeps_its_archive_unchanged_by_background_metadata_work() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let before = serde_json::to_value(
        state
            .repository_replica
            .lock()
            .await
            .snapshot_for_persistence(),
    )
    .unwrap();
    super::super::sequence_summary::run_maintenance_cycle(&state).await;
    let after = serde_json::to_value(
        state
            .repository_replica
            .lock()
            .await
            .snapshot_for_persistence(),
    )
    .unwrap();
    assert_eq!(
        after, before,
        "a non-repository must not migrate repository metadata or payloads"
    );
    let local_id = RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap();
    let identity = derived_repository_identity(&state, local_id).unwrap();
    let member = RepositoryMember::new(identity, RepositoryCapacity::default()).unwrap();
    state.store.lock().await.state_mut().repository_membership =
        Some(RepositoryMembership::new(vec![member]).unwrap());
    super::super::sequence_summary::run_maintenance_cycle(&state).await;
    let configured = serde_json::to_value(
        state
            .repository_replica
            .lock()
            .await
            .snapshot_for_persistence(),
    )
    .unwrap();
    assert_ne!(
        configured, before,
        "configured repository maintenance must remain active"
    );
}
