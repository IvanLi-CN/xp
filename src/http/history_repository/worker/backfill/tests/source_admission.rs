use super::super::*;
use crate::state::history_repository::control::{
    RepositoryCapacity, RepositoryMember, RepositoryMembership,
};

#[path = "app_state.rs"]
mod fixture;

#[tokio::test]
async fn non_repository_source_does_not_wait_for_repository_backfills() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let peer_id = xp_test_fixtures::secondary_node_id();
    {
        let mut store = state.store.lock().await;
        let mut peer = store.list_nodes().into_iter().next().unwrap();
        peer.node_id = peer_id.to_owned();
        store.upsert_node(peer).unwrap();
    }
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .initial_peer_backfill_checkpoint(peer_id)
            .is_none()
    );
    assert!(
        !initial_history_source_backfill_pending(&state).await,
        "a normal source cannot import repository baselines and must keep delivering its journal"
    );
    let local_identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let local = RepositoryMember::new(local_identity, RepositoryCapacity::default()).unwrap();
    state.store.lock().await.state_mut().repository_membership =
        Some(RepositoryMembership::new(vec![local]).unwrap());
    assert!(
        initial_history_source_backfill_pending(&state).await,
        "a configured repository must still prioritize the missing initial source baseline"
    );
    let remote_identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(peer_id.to_owned()).unwrap(),
    )
    .unwrap();
    let remote = RepositoryMember::new(remote_identity, RepositoryCapacity::default()).unwrap();
    state.store.lock().await.state_mut().repository_membership =
        Some(RepositoryMembership::new(vec![remote]).unwrap());
    assert!(
        !initial_history_source_backfill_pending(&state).await,
        "former repositories must resume source delivery without being re-added as repositories"
    );
}
