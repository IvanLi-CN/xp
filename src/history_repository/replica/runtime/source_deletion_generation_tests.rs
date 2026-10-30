use super::*;
use crate::node_history::{NodeHistoryHandle, RepositoryHistoryDeletionMarker};
use crate::state::history_repository::replica::runtime::tests::{identity, load, signing_key};

fn tombstone(marker: &RepositoryHistoryDeletionMarker) -> SyncRecord {
    SyncRecord::new(
        "node-a",
        "node-a",
        &marker.schema_id,
        1,
        marker.record_key.clone(),
        serde_json::to_vec(&serde_json::json!({"deletion_generation": marker.generation})).unwrap(),
        true,
    )
}

#[tokio::test]
async fn new_deletion_generation_requires_its_own_real_receipt_after_restart() {
    let source_dir = tempfile::tempdir().unwrap();
    let receiver_dir = tempfile::tempdir().unwrap();
    let history = NodeHistoryHandle::new(source_dir.path().join("node_history_cache.json"));
    let key = signing_key();
    let identity = identity(&key);
    let ready = vec!["repo-a".to_owned()];
    history.clear_user("target").await.unwrap();
    let old = history.repository_deletion_markers("local").await.unwrap();
    let mut source = load(source_dir.path());
    let first = source
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            identity.clone(),
            &key,
            old.iter().map(tombstone),
            100,
            &ready,
        )
        .unwrap();
    assert_eq!(first.len(), 1);
    let mut receiver = load(receiver_dir.path());
    let receipt = receiver
        .receive_wire_from_repository(
            "cluster-a",
            &identity,
            &first[0].wire,
            100,
            &ready,
            "repo-a",
        )
        .unwrap();
    source
        .acknowledge_tombstones(receipt.tombstone_acknowledgements())
        .unwrap();
    source
        .acknowledge_local_source_segment(&first[0].wire)
        .unwrap();
    assert!(
        source
            .local_source_tombstones_fully_acknowledged("node-a", &old)
            .unwrap()
    );

    history.clear_user("target").await.unwrap();
    let new = history.repository_deletion_markers("local").await.unwrap();
    let second = source
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            identity.clone(),
            &key,
            new.iter().map(tombstone),
            101,
            &ready,
        )
        .unwrap();
    assert_eq!(
        second.len(),
        1,
        "new generation must not be swallowed by same-key dedup"
    );
    assert_ne!(first[0].wire, second[0].wire);
    assert_eq!(
        source.snapshot.local_source.deletion_marker_keys.len(),
        new.len(),
        "superseded receipt mappings must not accumulate"
    );
    let decoded = SignedSegment::from_wire(&second[0].wire).unwrap();
    assert_eq!(decoded.canonical().first_cursor().sequence(), 1);
    assert_eq!(
        decoded.canonical().previous_segment_hash(),
        Some(
            SignedSegment::from_wire(&first[0].wire)
                .unwrap()
                .segment_hash()
                .unwrap()
        )
    );
    assert!(
        !source
            .local_source_tombstones_fully_acknowledged("node-a", &new)
            .unwrap()
    );
    source.complete_local_source_tombstones(&old).unwrap();
    history
        .complete_repository_deletion_marker("local", &old[0])
        .await
        .unwrap();
    drop(source);
    drop(receiver);
    let mut source = load(source_dir.path());
    let mut receiver = load(receiver_dir.path());
    let retry = source
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            identity.clone(),
            &key,
            new.iter().map(tombstone),
            102,
            &ready,
        )
        .unwrap();
    assert_eq!(retry.len(), 1);
    assert_eq!(
        retry[0].wire, second[0].wire,
        "restart replays the exact original wire"
    );
    assert_eq!(source.local_source_next_sequence("tombstone"), Some(2));
    source
        .acknowledge_tombstones(receipt.tombstone_acknowledgements())
        .unwrap();
    assert!(
        !source
            .local_source_tombstones_fully_acknowledged("node-a", &new)
            .unwrap(),
        "old receipt cannot satisfy the new generation"
    );
    let receipt = receiver
        .receive_wire_from_repository(
            "cluster-a",
            &identity,
            &retry[0].wire,
            102,
            &ready,
            "repo-a",
        )
        .unwrap();
    source
        .acknowledge_tombstones(receipt.tombstone_acknowledgements())
        .unwrap();
    assert!(
        source
            .local_source_tombstones_fully_acknowledged("node-a", &new)
            .unwrap()
    );
    source
        .acknowledge_local_source_segment(&retry[0].wire)
        .unwrap();
    history
        .complete_repository_deletion_marker("local", &new[0])
        .await
        .unwrap();
    source.complete_local_source_tombstones(&new).unwrap();
    drop(source);
    assert!(
        load(source_dir.path())
            .local_source_pending_segments_page()
            .is_empty()
    );
    assert!(
        history
            .repository_deletion_markers("local")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn interrupted_old_generation_keeps_journal_and_retires_superseded_mapping() {
    let directory = tempfile::tempdir().unwrap();
    let history = NodeHistoryHandle::new(directory.path().join("node_history_cache.json"));
    let key = signing_key();
    history.clear_user("target").await.unwrap();
    let old = history.repository_deletion_markers("local").await.unwrap();
    let mut source = load(directory.path());
    let first = source
        .queue_local_source_segments(
            "cluster-a",
            identity(&key),
            &key,
            &old.iter().map(tombstone).collect::<Vec<_>>(),
            100,
        )
        .unwrap();
    assert_eq!(first.len(), 1);
    history.clear_user("target").await.unwrap();
    let new = history.repository_deletion_markers("local").await.unwrap();
    drop(source);
    let mut source = load(directory.path());
    let pending = source
        .queue_local_source_segments(
            "cluster-a",
            identity(&key),
            &key,
            &new.iter().map(tombstone).collect::<Vec<_>>(),
            101,
        )
        .unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].wire, first[0].wire);
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 1);
    drop(source);
    let source = load(directory.path());
    assert_eq!(source.local_source_pending_segments_page().len(), 2);
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 1);
}

#[tokio::test]
async fn failed_marker_completion_retains_durable_source_receipt_mapping() {
    let directory = tempfile::tempdir().unwrap();
    let history = NodeHistoryHandle::new(directory.path().join("node_history_cache.json"));
    history.clear_user("target").await.unwrap();
    let markers = history.repository_deletion_markers("local").await.unwrap();
    let mut source = load(directory.path());
    let key = signing_key();
    source
        .queue_local_source_segments(
            "cluster-a",
            identity(&key),
            &key,
            &markers.iter().map(tombstone).collect::<Vec<_>>(),
            100,
        )
        .unwrap();
    let before = source.snapshot.local_source.deletion_marker_keys.clone();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_marker_completion BEFORE INSERT ON history_snapshots
         WHEN NEW.key = 'repository_replica' BEGIN
         SELECT RAISE(ABORT, 'injected completion failure'); END;",
        )
        .unwrap();
    assert!(source.complete_local_source_tombstones(&markers).is_err());
    assert_eq!(source.snapshot.local_source.deletion_marker_keys, before);
    drop(source);
    let mut source = load(directory.path());
    assert_eq!(source.snapshot.local_source.deletion_marker_keys, before);
    connection
        .execute_batch("DROP TRIGGER reject_marker_completion")
        .unwrap();
    source.complete_local_source_tombstones(&markers).unwrap();
    drop(source);
    assert!(
        load(directory.path())
            .snapshot
            .local_source
            .deletion_marker_keys
            .is_empty()
    );
}
