use super::*;
use crate::node_history::RepositoryHistoryDeletionMarker;

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
