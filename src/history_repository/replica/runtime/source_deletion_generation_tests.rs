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

#[tokio::test]
async fn stale_intent_snapshot_cannot_prune_a_newer_generation() {
    let directory = tempfile::tempdir().unwrap();
    let history = NodeHistoryHandle::new(directory.path().join("node_history_cache.json"));
    let (version, old_markers) = history.repository_deletion_snapshot("local").await.unwrap();
    assert!(old_markers.is_empty());
    history.clear_user("target").await.unwrap();
    let new_markers = history.repository_deletion_markers("local").await.unwrap();
    let key = signing_key();
    let mut source = load(directory.path());
    let segments = source
        .queue_local_source_segments(
            "cluster-a",
            identity(&key),
            &key,
            &new_markers.iter().map(tombstone).collect::<Vec<_>>(),
            100,
        )
        .unwrap();
    source
        .reconcile_local_source_tombstones("node-a", &old_markers, version)
        .unwrap();
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 1);
    assert_eq!(
        source.local_source_pending_segments_page()[0].wire,
        segments[0].wire
    );
}

#[tokio::test]
async fn bounded_orphan_scan_passes_live_entries_and_keeps_unacked_wires() {
    let directory = tempfile::tempdir().unwrap();
    let history = NodeHistoryHandle::new(directory.path().join("node_history_cache.json"));
    for index in 0..129 {
        history
            .clear_user(&format!("active-{index:03}"))
            .await
            .unwrap();
    }
    let (version, markers) = history.repository_deletion_snapshot("local").await.unwrap();
    let mut records = markers.iter().map(tombstone).collect::<Vec<_>>();
    records.extend((0..129).map(|index| {
        SyncRecord::new(
            "node-a",
            "node-a",
            "traffic.v1",
            1,
            format!("node-history:user:orphan-{index:03}:").into_bytes(),
            b"{}".to_vec(),
            true,
        )
    }));
    let key = signing_key();
    let mut source = load(directory.path());
    let segments = source
        .queue_local_source_segments("cluster-a", identity(&key), &key, &records, 100)
        .unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 258);
    source
        .reconcile_local_source_tombstones("node-a", &markers, version)
        .unwrap();
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 258);
    source
        .reconcile_local_source_tombstones("node-a", &markers, version)
        .unwrap();
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 131);
    drop(source);
    let mut source = load(directory.path());
    for _ in 0..3 {
        source
            .reconcile_local_source_tombstones("node-a", &markers, version)
            .unwrap();
    }
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 129);
    assert_eq!(
        source.local_source_pending_segments_page()[0].wire,
        segments[0].wire
    );
}

#[tokio::test]
async fn failed_orphan_reconcile_keeps_mapping_and_wire_for_retry() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let record = SyncRecord::new(
        "node-a",
        "node-a",
        "traffic.v1",
        1,
        b"node-history:user:orphan:".to_vec(),
        b"{}".to_vec(),
        true,
    );
    let mut source = load(directory.path());
    let segments = source
        .queue_local_source_segments("cluster-a", identity(&key), &key, &[record], 100)
        .unwrap();
    let (id, stored) = source
        .snapshot
        .local_source
        .deletion_marker_keys
        .iter()
        .next()
        .unwrap();
    assert_eq!(stored.source_node_id(), "node-a");
    assert!(
        source.snapshot.local_source.node_id.is_empty()
            || source.snapshot.local_source.node_id == "node-a"
    );
    assert_eq!(stored.source_epoch(), source.snapshot.local_source.epoch);
    assert_eq!(stored.stream(), "tombstone");
    assert_eq!(marker_mapping_generation(id, stored), Some(0));
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_orphan_cleanup BEFORE INSERT ON history_snapshots
             WHEN NEW.key = 'repository_replica' BEGIN
             SELECT RAISE(ABORT, 'injected orphan cleanup failure'); END;",
        )
        .unwrap();
    assert!(
        source
            .reconcile_local_source_tombstones("node-a", &[], 0)
            .is_err()
    );
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 1);
    drop(source);
    let mut source = load(directory.path());
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 1);
    assert_eq!(
        source.local_source_pending_segments_page()[0].wire,
        segments[0].wire
    );
    connection
        .execute_batch("DROP TRIGGER reject_orphan_cleanup")
        .unwrap();
    source
        .reconcile_local_source_tombstones("node-a", &[], 0)
        .unwrap();
    drop(source);
    let source = load(directory.path());
    assert!(source.snapshot.local_source.deletion_marker_keys.is_empty());
    assert_eq!(
        source.local_source_pending_segments_page()[0].wire,
        segments[0].wire
    );
}

#[tokio::test]
async fn orphan_scan_preserves_unknown_tombstone_mapping() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let record = SyncRecord::new(
        "node-a",
        "node-a",
        "traffic.v1",
        1,
        b"unknown-tombstone".to_vec(),
        b"{}".to_vec(),
        true,
    );
    let mut source = load(directory.path());
    let segments = source
        .queue_local_source_segments("cluster-a", identity(&key), &key, &[record], 100)
        .unwrap();
    source
        .reconcile_local_source_tombstones("node-a", &[], 0)
        .unwrap();
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 1);
    assert_eq!(
        source.local_source_pending_segments_page()[0].wire,
        segments[0].wire
    );
}

#[tokio::test]
async fn orphan_scan_preserves_mapping_outside_the_current_source_epoch() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let record = SyncRecord::new(
        "node-a",
        "node-a",
        "traffic.v1",
        1,
        b"node-history:user:foreign:".to_vec(),
        b"{}".to_vec(),
        true,
    );
    let mut source = load(directory.path());
    let segments = source
        .queue_local_source_segments("cluster-a", identity(&key), &key, &[record], 100)
        .unwrap();
    let cursor = ReplicaCursor::new("other-source", 1, "tombstone", 0).unwrap();
    let foreign = ReplicaRecord::new(
        &cursor,
        "node-a",
        "node-a",
        "traffic.v1",
        1,
        b"node-history:user:foreign:".to_vec(),
        b"{}".to_vec(),
    )
    .unwrap()
    .key();
    let (_, value) = source
        .snapshot
        .local_source
        .deletion_marker_keys
        .iter_mut()
        .next()
        .unwrap();
    *value = foreign;
    source
        .reconcile_local_source_tombstones("node-a", &[], u64::MAX)
        .unwrap();
    assert_eq!(source.snapshot.local_source.deletion_marker_keys.len(), 1);
    assert_eq!(
        source.local_source_pending_segments_page()[0].wire,
        segments[0].wire
    );
}

#[tokio::test]
async fn orphan_scan_recovers_older_epoch_but_preserves_a_future_epoch() {
    for future in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let key = signing_key();
        let record = SyncRecord::new(
            "node-a",
            "node-a",
            "traffic.v1",
            1,
            b"node-history:user:epoch:".to_vec(),
            b"{}".to_vec(),
            true,
        );
        let mut source = load(directory.path());
        let segments = source
            .queue_local_source_segments("cluster-a", identity(&key), &key, &[record], 100)
            .unwrap();
        let epoch = if future {
            source.snapshot.local_source.epoch + 1
        } else {
            0
        };
        let cursor = ReplicaCursor::new("node-a", epoch, "tombstone", 0).unwrap();
        let mapped = ReplicaRecord::new(
            &cursor,
            "node-a",
            "node-a",
            "traffic.v1",
            1,
            b"node-history:user:epoch:".to_vec(),
            b"{}".to_vec(),
        )
        .unwrap()
        .key();
        let (_, value) = source
            .snapshot
            .local_source
            .deletion_marker_keys
            .iter_mut()
            .next()
            .unwrap();
        *value = mapped;
        source
            .reconcile_local_source_tombstones("node-a", &[], u64::MAX)
            .unwrap();
        assert_eq!(
            source.snapshot.local_source.deletion_marker_keys.len(),
            usize::from(future)
        );
        assert_eq!(
            source.local_source_pending_segments_page()[0].wire,
            segments[0].wire
        );
    }
}
