use super::super::*;
use crate::{
    internal_auth::InternalRoute,
    state::history_repository::{
        HistoryStorage,
        control::{RepositoryCapacity, RepositoryMember, RepositoryMembership},
        replica::{RepositoryReplicaRuntime, RepositoryReplicaSegment},
    },
};
use sha2::Sha256;

#[path = "app_state.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "legacy_recovery_peer.rs"]
mod peer;

const NOW: u64 = 1_800_000_000;
const RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const PEER: &str = "registered-syncing-peer";

async fn configure_membership(state: &AppState, now: u64) {
    let members = [&state.cluster.node_id, &PEER.to_owned()]
        .into_iter()
        .map(|id| {
            let identity =
                derived_repository_identity(state, RepositoryNodeId::try_from(id.clone()).unwrap())
                    .unwrap();
            RepositoryMember::new(identity, RepositoryCapacity::default()).unwrap()
        })
        .collect();
    let mut membership = RepositoryMembership::new(members).unwrap();
    let peer = RepositoryNodeId::try_from(PEER.to_owned()).unwrap();
    membership.mark_catch_up_complete(&peer, now - 300).unwrap();
    membership.mark_ready(&peer, now).unwrap();
    let mut store = state.store.lock().await;
    store.state_mut().repository_membership = Some(membership);
    store.save().unwrap();
}

async fn seed_pending_first_page(state: &AppState, now: chrono::DateTime<chrono::Utc>) {
    let count = MAX_INITIAL_BACKFILL_PAGE_RECORDS * 2 + 1;
    let snapshot = state.node_runtime.snapshot(0).await;
    for index in 0..count {
        state
            .node_history
            .record_local_sample(
                now - chrono::Duration::minutes(((count - index) * 5) as i64),
                &state.cluster.node_id,
                Some(Vec::new()),
                snapshot.clone(),
            )
            .await;
    }
    let mut history = state
        .node_history
        .snapshot(&state.cluster.node_id)
        .await
        .unwrap();
    history.daily_traffic.clear();
    history.daily_component_status.clear();
    history.component_status_events.clear();
    history.traffic.as_mut().unwrap().daily.clear();
    state
        .node_history
        .replace_node_snapshot(now, &state.cluster.node_id, history)
        .await;
    let collected =
        historical_source_backfill_records(state, None, MAX_INITIAL_BACKFILL_PAGE_RECORDS)
            .await
            .unwrap();
    let next = collected
        .next_cursor()
        .unwrap()
        .expect("three-page baseline");
    let batches = queued_history_backfill_batches(
        historical_record_batches(collected.into_records()).unwrap(),
    );
    let id = RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap();
    state
        .repository_replica
        .lock()
        .await
        .queue_local_history_backfill_batches_at(
            &state.cluster.cluster_id,
            derived_repository_identity(state, id).unwrap(),
            &derived_repository_signing_key(state, &state.cluster.node_id).unwrap(),
            Some(next),
            false,
            batches,
            now.timestamp() as u64,
        )
        .unwrap();
}

async fn local_member(state: &AppState) -> RepositoryMember {
    state
        .store
        .lock()
        .await
        .state()
        .repository_membership
        .as_ref()
        .unwrap()
        .repository(&RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap())
        .unwrap()
        .clone()
}

#[tokio::test]
async fn ready_catch_up_drains_pending_local_continuations_across_restart_before_ready() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = fixture::app_state(directory.path());
    let now = chrono::Utc::now();
    let seconds = now.timestamp() as u64;
    let server = peer::Peer::start(&state, &[]).await;
    server.register(&state).await;
    configure_membership(&state, seconds).await;
    seed_pending_first_page(&state, now).await;
    let storage = HistoryStorage::open(directory.path());
    super::super::super::advance_local_repository_lifecycle(&state, seconds)
        .await
        .unwrap();
    assert_eq!(storage.repository_history_record_count().unwrap(), 128);
    assert!(local_member(&state).await.catch_up_completed_at().is_none());
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    // Restart with the durable continuation cursor, but no in-flight page.
    drop(state);
    state = fixture::app_state(directory.path());
    super::super::super::advance_local_repository_lifecycle(&state, seconds + 5)
        .await
        .unwrap();
    assert_eq!(
        storage.repository_history_record_count().unwrap(),
        256,
        "Ready peer completion must not skip the second local page"
    );
    assert!(local_member(&state).await.catch_up_completed_at().is_none());
    super::super::super::advance_local_repository_lifecycle(&state, seconds + 10)
        .await
        .unwrap();
    assert_eq!(storage.repository_history_record_count().unwrap(), 257);
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
    assert!(local_member(&state).await.catch_up_completed_at().is_none());
    super::super::super::advance_local_repository_lifecycle(&state, seconds + 15)
        .await
        .unwrap();
    assert_eq!(
        local_member(&state).await.catch_up_completed_at(),
        Some(seconds + 15)
    );
    drop(state);
    state = fixture::app_state(directory.path());
    super::super::super::advance_local_repository_lifecycle(&state, seconds + 314)
        .await
        .unwrap();
    assert_eq!(
        *local_member(&state).await.lifecycle(),
        RepositoryLifecycle::Syncing
    );
    super::super::super::advance_local_repository_lifecycle(&state, seconds + 315)
        .await
        .unwrap();
    assert_eq!(
        *local_member(&state).await.lifecycle(),
        RepositoryLifecycle::Ready
    );
    assert_eq!(storage.repository_history_record_count().unwrap(), 257);
    let rows = storage
        .repository_history_records(None, None, None, 0, 300)
        .unwrap();
    assert!(
        rows.iter()
            .all(|row| row.stream == format!("traffic-backfill-{}", state.cluster.node_id))
    );
    assert_eq!(
        rows.iter()
            .map(|row| &row.record_key)
            .collect::<BTreeSet<_>>()
            .len(),
        257
    );
    assert!(storage.source_delivery_journal().unwrap().is_empty());
    let requests = server.requests().await;
    assert!(requests.iter().any(|path| path.contains("/summary")));
    assert!(
        requests
            .iter()
            .any(|path| path.contains("/initial-backfill"))
    );
}

#[tokio::test]
async fn stale_raft_completion_cannot_make_unfinished_local_pages_ready() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let now = chrono::Utc::now();
    let seconds = now.timestamp() as u64;
    let server = peer::Peer::start(&state, &[]).await;
    server.register(&state).await;
    configure_membership(&state, seconds).await;
    seed_pending_first_page(&state, now).await;
    let local = RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap();
    {
        let mut store = state.store.lock().await;
        store
            .state_mut()
            .repository_membership
            .as_mut()
            .unwrap()
            .mark_catch_up_complete(&local, seconds - 300)
            .unwrap();
        store.save().unwrap();
    }
    drop(state);
    let state = fixture::app_state(directory.path());
    super::super::super::advance_local_repository_lifecycle(&state, seconds)
        .await
        .unwrap();
    assert_eq!(
        HistoryStorage::open(directory.path())
            .repository_history_record_count()
            .unwrap(),
        128
    );
    let member = local_member(&state).await;
    assert_eq!(*member.lifecycle(), RepositoryLifecycle::Syncing);
    assert!(member.catch_up_completed_at().is_none());
}

#[tokio::test]
async fn ready_peer_bootstrap_does_not_start_an_unqueued_local_export() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let now = chrono::Utc::now().timestamp() as u64;
    let server = peer::Peer::start(&state, &[]).await;
    server.register(&state).await;
    configure_membership(&state, now).await;
    super::super::super::advance_local_repository_lifecycle(&state, now)
        .await
        .unwrap();
    assert_eq!(
        local_member(&state).await.catch_up_completed_at(),
        Some(now)
    );
    let runtime = state.repository_replica.lock().await;
    assert!(!runtime.local_history_backfill_completed());
    assert!(runtime.local_history_backfill_cursor().is_none());
    assert!(
        runtime
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
}

#[tokio::test]
async fn syncing_local_tombstone_receipt_survives_runtime_reconcile_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let now = chrono::Utc::now().timestamp() as u64;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
    let local = state.cluster.node_id.clone();
    let expected = {
        let mut runtime = state.repository_replica.lock().await;
        let segment = runtime
            .queue_local_source_segment(
                &state.cluster.cluster_id,
                identity.clone(),
                &key,
                vec![crate::history_sync::SyncRecord::new(
                    &local,
                    &local,
                    "traffic.v1",
                    1,
                    b"node-history:node:deleted:".to_vec(),
                    Vec::new(),
                    true,
                )],
                now,
            )
            .unwrap()
            .unwrap();
        let receipt = runtime
            .receive_wire_from_repository(
                &state.cluster.cluster_id,
                &identity,
                &segment.wire,
                now,
                &[PEER.to_owned(), local.clone()],
                &local,
            )
            .unwrap();
        assert_eq!(receipt.tombstone_acknowledgements().len(), 1);
        let expected = serde_json::to_value(receipt.tombstone_acknowledgements()).unwrap();
        runtime
            .reconcile_ready_repositories(&[PEER.to_owned()])
            .unwrap();
        expected
    };
    drop(state);
    let state = fixture::app_state(directory.path());
    let mut runtime = state.repository_replica.lock().await;
    let page = runtime.tombstone_acknowledgement_page(&local).unwrap();
    assert_eq!(
        serde_json::to_value(page.acknowledgements()).unwrap(),
        expected
    );
    runtime
        .reconcile_ready_repositories(&[PEER.to_owned(), local.clone(), "new-ready".into()])
        .unwrap();
    assert!(
        runtime
            .tombstone_acknowledgement_page("new-ready")
            .unwrap()
            .acknowledgements()
            .is_empty()
    );
}
