use super::*;
use crate::node_history::RepositoryHistoryDeletionMarker;

#[tokio::test]
async fn admin_delete_recovers_all_destinations_after_activation_failure() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let (user, destinations) = {
        let mut store = state.store.lock().await;
        let user = store.create_user("api recovery".to_owned(), None).unwrap();
        let template = store.get_node(&state.cluster.node_id).unwrap();
        let mut destinations = Vec::new();
        for _ in 0..2 {
            let mut node = template.clone();
            node.node_id = crate::id::new_ulid_string();
            destinations.push(node.node_id.clone());
            store.upsert_node(node).unwrap();
        }
        (user, destinations)
    };
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_activated_fanout BEFORE INSERT ON history_snapshots
         WHEN NEW.key = 'node_history'
          AND CAST(NEW.payload AS TEXT) NOT LIKE '%prepared_user_deletions%'
         BEGIN SELECT RAISE(ABORT, 'injected activation failure'); END;",
        )
        .unwrap();
    let error = crate::http::admin_delete_user(
        axum::Extension(state.clone()),
        axum::extract::Path(user.user_id.clone()),
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert!(state.store.lock().await.get_user(&user.user_id).is_none());
    drop(state);
    connection
        .execute_batch("DROP TRIGGER reject_activated_fanout")
        .unwrap();
    let state = fixture::app_state(directory.path());
    assert_eq!(
        state
            .node_history
            .recover_prepared_user_deletions(
                &state.store,
                state.raft.as_ref(),
                &state.cluster.node_id
            )
            .await
            .unwrap(),
        1
    );
    for destination in destinations {
        assert_eq!(
            state
                .node_history
                .repository_deletion_markers(&destination)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn prepared_user_deletion_recovers_raft_and_all_destinations_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let user = state
        .store
        .lock()
        .await
        .create_user("recover".to_owned(), None)
        .unwrap();
    let destinations = vec!["peer-b".to_owned(), "peer-c".to_owned()];
    state
        .node_history
        .prepare_user_deletion(&user.user_id, &destinations)
        .await
        .unwrap();
    assert!(
        state
            .node_history
            .repository_deletion_markers("peer-c")
            .await
            .unwrap()
            .is_empty(),
        "preparation must not delete history before Raft commits"
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    let (first, second) = tokio::join!(
        state.node_history.recover_prepared_user_deletions(
            &state.store,
            state.raft.as_ref(),
            &state.cluster.node_id
        ),
        state.node_history.recover_prepared_user_deletions(
            &state.store,
            state.raft.as_ref(),
            &state.cluster.node_id
        ),
    );
    assert_eq!(
        first.unwrap() + second.unwrap(),
        1,
        "concurrent recovery must activate once"
    );
    assert!(state.store.lock().await.get_user(&user.user_id).is_none());
    let ready = vec![state.cluster.node_id.clone()];
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &ready,
        None,
        &[],
        100,
        true,
    )
    .await
    .unwrap();
    assert!(
        HistoryStorage::open(directory.path())
            .source_delivery_journal()
            .unwrap()
            .is_empty(),
        "the local receiver must really ACK the wire"
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    for destination in &destinations {
        assert_eq!(
            state
                .node_history
                .repository_deletion_markers(destination)
                .await
                .unwrap()
                .len(),
            1,
            "every destination remains retryable after ACK and restart"
        );
    }
}

#[tokio::test]
async fn failed_fanout_activation_is_recovered_after_raft_commit_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let user = state
        .store
        .lock()
        .await
        .create_user("recover failed write".to_owned(), None)
        .unwrap();
    let destinations = vec!["peer-b".to_owned(), "peer-c".to_owned()];
    state
        .node_history
        .prepare_user_deletion(&user.user_id, &destinations)
        .await
        .unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_fanout BEFORE INSERT ON history_snapshots
             WHEN NEW.key = 'node_history' BEGIN
             SELECT RAISE(ABORT, 'injected fanout failure'); END;",
        )
        .unwrap();
    assert!(
        state
            .node_history
            .recover_prepared_user_deletions(
                &state.store,
                state.raft.as_ref(),
                &state.cluster.node_id
            )
            .await
            .is_err()
    );
    assert!(state.store.lock().await.get_user(&user.user_id).is_none());
    assert!(
        state
            .node_history
            .repository_deletion_markers("peer-c")
            .await
            .unwrap()
            .is_empty(),
        "a failed activation must not expose a partial fanout"
    );
    drop(state);
    connection
        .execute_batch("DROP TRIGGER reject_fanout")
        .unwrap();
    let state = fixture::app_state(directory.path());
    assert_eq!(
        state
            .node_history
            .recover_prepared_user_deletions(
                &state.store,
                state.raft.as_ref(),
                &state.cluster.node_id
            )
            .await
            .unwrap(),
        1
    );
    for destination in &destinations {
        assert_eq!(
            state
                .node_history
                .repository_deletion_markers(destination)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn large_deletion_backlog_is_captured_in_bounded_pages() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let users = (0..1001)
        .map(|index| format!("user-{index:04}"))
        .collect::<Vec<_>>();
    let nodes = (0..26)
        .map(|index| format!("node-{index:04}"))
        .collect::<Vec<_>>();
    let snapshot =
        serde_json::json!({ "schema_version": 2, "deleted_users": users, "deleted_nodes": nodes });
    storage
        .write(
            crate::state::history_repository::NODE_HISTORY_KEY,
            &serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    drop(storage);
    let state = fixture::app_state(directory.path());
    let batch = super::super::super::super::source_records::source_records(&state, 100)
        .await
        .unwrap();
    assert!(
        batch.deletion_markers.len() <= 128,
        "capture must remain bounded for a large backlog"
    );
    assert!(
        batch.deletion_snapshot_version.is_none(),
        "a partial page is not a complete deletion snapshot"
    );
    for marker in &batch.deletion_markers {
        if let Some(node) = marker.target_node_id() {
            assert_eq!(
                batch
                    .deletion_markers
                    .iter()
                    .filter(|candidate| candidate.target_node_id() == Some(node))
                    .count(),
                5
            );
        }
    }
    let ready = vec![state.cluster.node_id.clone()];
    let unreachable = vec!["unreachable".to_owned()];
    let later = state
        .node_history
        .repository_deletion_markers(&state.cluster.node_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
    let record = SyncRecord::new(
        &state.cluster.node_id,
        &state.cluster.node_id,
        &later.schema_id,
        1,
        later.record_key.clone(),
        b"{}".to_vec(),
        true,
    );
    state
        .repository_replica
        .lock()
        .await
        .queue_local_source_segments_for_repositories(
            &state.cluster.cluster_id,
            identity,
            &key,
            [record],
            100,
            &ready,
        )
        .unwrap();
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &ready,
        Some(&unreachable),
        &[],
        100,
        true,
    )
    .await
    .unwrap();
    assert_eq!(
        mapping_count(directory.path()),
        batch.deletion_markers.len() + 1,
        "a partial page must not prune a mapping for a later deletion"
    );
    assert!(
        !HistoryStorage::open(directory.path())
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
    state
        .node_history
        .clear_user("a-new-earlier-user")
        .await
        .unwrap();
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap()
            .len(),
        1132,
        "unacknowledged intent survives restart"
    );
    for tick in 0..20 {
        crate::http::history_repository::worker::publish_local_history_segment(
            &state,
            &ready,
            None,
            &[],
            101 + tick,
            true,
        )
        .await
        .unwrap();
        if state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap()
            .is_empty()
        {
            break;
        }
    }
    assert!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap()
            .is_empty(),
        "all deletion intents must converge after the repository returns"
    );
}

#[tokio::test]
async fn large_node_deletion_splits_signed_segments_without_losing_schemas() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    state
        .node_history
        .clear_node(&"n".repeat(19_000))
        .await
        .unwrap();
    let ready = vec![state.cluster.node_id.clone()];
    let unreachable = vec!["unreachable".to_owned()];
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &ready,
        Some(&unreachable),
        &[],
        100,
        true,
    )
    .await
    .unwrap();
    let journal = HistoryStorage::open(directory.path())
        .source_delivery_journal()
        .unwrap();
    let tombstones = journal
        .iter()
        .filter(|row| row.stream == "tombstone")
        .collect::<Vec<_>>();
    assert!(tombstones.len() > 1);
    let mut record_count = 0;
    for row in tombstones {
        let segment = SignedSegment::from_wire(&row.wire).unwrap();
        assert!(segment.canonical().canonical_bytes().unwrap().len() <= 192 * 1024);
        record_count += segment.canonical().records().len();
    }
    assert_eq!(record_count, 5);
    drop(state);
    let state = fixture::app_state(directory.path());
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &ready,
        None,
        &[],
        101,
        true,
    )
    .await
    .unwrap();
    assert!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn acknowledged_deletion(state: &AppState) -> Vec<RepositoryHistoryDeletionMarker> {
    state.node_history.clear_user("target").await.unwrap();
    let markers = state
        .node_history
        .repository_deletion_markers(&state.cluster.node_id)
        .await
        .unwrap();
    let identity = derived_repository_identity(
        state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let key = derived_repository_signing_key(state, &state.cluster.node_id).unwrap();
    let ready = vec![state.cluster.node_id.clone()];
    let records = markers
        .iter()
        .map(|marker| {
            SyncRecord::new(
                &state.cluster.node_id,
                &state.cluster.node_id,
                &marker.schema_id,
                1,
                marker.record_key.clone(),
                serde_json::to_vec(&serde_json::json!({
                    "deletion_generation": marker.generation,
                }))
                .unwrap(),
                true,
            )
        })
        .collect::<Vec<_>>();
    let mut source = state.repository_replica.lock().await;
    let segments = source
        .queue_local_source_segments_for_repositories(
            &state.cluster.cluster_id,
            identity.clone(),
            &key,
            records,
            100,
            &ready,
        )
        .unwrap();
    assert_eq!(segments.len(), 1);
    let receipt = source
        .receive_wire_from_repository(
            &state.cluster.cluster_id,
            &identity,
            &segments[0].wire,
            100,
            &ready,
            &state.cluster.node_id,
        )
        .unwrap();
    source
        .acknowledge_tombstones(receipt.tombstone_acknowledgements())
        .unwrap();
    source
        .acknowledge_local_source_segment(&segments[0].wire)
        .unwrap();
    assert!(
        source
            .local_source_tombstones_fully_acknowledged(&state.cluster.node_id, &markers)
            .unwrap()
    );
    markers
}

fn mapping_count(directory: &std::path::Path) -> usize {
    let storage = HistoryStorage::open(directory);
    let bytes = storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap();
    let snapshot: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    snapshot["local_source"]["deletion_marker_keys"]
        .as_object()
        .map_or(0, |mapping| mapping.len())
}

#[tokio::test]
async fn ack_cleanup_failure_preserves_intent_and_mapping_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let markers = acknowledged_deletion(&state).await;
    assert_eq!(mapping_count(directory.path()), 1);
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_receipt_cleanup BEFORE INSERT ON history_snapshots
             WHEN NEW.key = 'repository_replica' BEGIN
             SELECT RAISE(ABORT, 'injected receipt failure'); END;",
        )
        .unwrap();
    assert!(
        crate::http::history_repository::worker::source_delivery::complete_acked_source_deletions(
            &state, &markers,
        )
        .await
        .is_err()
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap(),
        markers,
        "an unsuccessful receipt cleanup must leave deletion intent available for retry"
    );
    assert_eq!(mapping_count(directory.path()), 1);
}

#[tokio::test]
async fn orphaned_acked_mapping_is_reconciled_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let markers = acknowledged_deletion(&state).await;
    for marker in &markers {
        state
            .node_history
            .complete_repository_deletion_marker(&state.cluster.node_id, marker)
            .await
            .unwrap();
    }
    assert_eq!(mapping_count(directory.path()), 1);
    drop(state);
    let state = fixture::app_state(directory.path());
    assert!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap()
            .is_empty()
    );
    let ready = vec![state.cluster.node_id.clone()];
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &ready,
        None,
        &[],
        110,
        true,
    )
    .await
    .unwrap();
    drop(state);
    assert_eq!(
        mapping_count(directory.path()),
        0,
        "completed receipt mapping must converge"
    );
}

#[tokio::test]
async fn intent_write_failure_retries_after_mapping_is_removed() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let markers = acknowledged_deletion(&state).await;
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_intent_cleanup BEFORE INSERT ON history_snapshots
             WHEN NEW.key = 'node_history' BEGIN
             SELECT RAISE(ABORT, 'injected intent failure'); END;",
        )
        .unwrap();
    assert!(
        crate::http::history_repository::worker::source_delivery::complete_acked_source_deletions(
            &state, &markers,
        )
        .await
        .is_err()
    );
    assert_eq!(mapping_count(directory.path()), 0);
    drop(state);
    connection
        .execute_batch("DROP TRIGGER reject_intent_cleanup")
        .unwrap();
    let state = fixture::app_state(directory.path());
    assert_eq!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap(),
        markers
    );
    let ready = vec![state.cluster.node_id.clone()];
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &ready,
        None,
        &[],
        110,
        true,
    )
    .await
    .unwrap();
    assert!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(mapping_count(directory.path()), 0);
}

#[tokio::test]
async fn orphan_cleanup_never_deletes_an_unacknowledged_wire() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    state.node_history.clear_user("target").await.unwrap();
    let markers = state
        .node_history
        .repository_deletion_markers(&state.cluster.node_id)
        .await
        .unwrap();
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
    let ready = vec!["unavailable-repository".to_owned()];
    let record = SyncRecord::new(
        &state.cluster.node_id,
        &state.cluster.node_id,
        &markers[0].schema_id,
        1,
        markers[0].record_key.clone(),
        serde_json::to_vec(&serde_json::json!({"deletion_generation": markers[0].generation}))
            .unwrap(),
        true,
    );
    let wire = state
        .repository_replica
        .lock()
        .await
        .queue_local_source_segments_for_repositories(
            &state.cluster.cluster_id,
            identity,
            &key,
            [record],
            100,
            &ready,
        )
        .unwrap()[0]
        .wire
        .clone();
    state
        .node_history
        .complete_repository_deletion_marker(&state.cluster.node_id, &markers[0])
        .await
        .unwrap();
    assert_eq!(mapping_count(directory.path()), 1);
    assert!(
        !crate::http::history_repository::worker::publish_local_history_segment(
            &state,
            &ready,
            None,
            &[],
            110,
            true,
        )
        .await
        .unwrap()
    );
    assert_eq!(mapping_count(directory.path()), 0);
    let journal = HistoryStorage::open(directory.path())
        .source_delivery_journal()
        .unwrap();
    assert!(journal.iter().any(|row| row.wire == wire));
}

#[tokio::test]
async fn older_ack_cleanup_does_not_complete_a_new_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let old = acknowledged_deletion(&state).await;
    state.node_history.clear_user("target").await.unwrap();
    let new = state
        .node_history
        .repository_deletion_markers(&state.cluster.node_id)
        .await
        .unwrap();
    assert_ne!(old, new);
    crate::http::history_repository::worker::source_delivery::complete_acked_source_deletions(
        &state, &old,
    )
    .await
    .unwrap();
    assert_eq!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap(),
        new
    );
    let ready = vec![state.cluster.node_id.clone()];
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &ready,
        None,
        &[],
        110,
        true,
    )
    .await
    .unwrap();
    assert!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn paused_capture_never_treats_empty_batch_as_intent_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let markers = acknowledged_deletion(&state).await;
    let ready = vec![state.cluster.node_id.clone()];
    assert!(
        !crate::http::history_repository::worker::publish_local_history_segment(
            &state,
            &ready,
            None,
            &[],
            110,
            false,
        )
        .await
        .unwrap()
    );
    assert_eq!(mapping_count(directory.path()), 1);
    assert_eq!(
        state
            .node_history
            .repository_deletion_markers(&state.cluster.node_id)
            .await
            .unwrap(),
        markers
    );
}
