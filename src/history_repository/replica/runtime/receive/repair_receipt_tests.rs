use super::super::super::tests::{identity, load, record, segment, signing_key};
use sha2::Digest as _;

#[test]
fn historical_repair_receipts_are_returned_only_after_atomic_commit() {
    let directory = tempfile::tempdir().expect("directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let second = segment(
        &key,
        1,
        vec![record(b"second", false)],
        Some(first.segment_hash().expect("hash")),
    );
    let mut runtime = load(directory.path());
    let receipts = runtime
        .receive_repair_wires_with_receipts(
            "cluster-a",
            &identity,
            &[
                (first.wire_bytes().expect("wire"), 11),
                (second.wire_bytes().expect("wire"), 12),
            ],
            &["local".to_owned()],
            "local",
        )
        .expect("commit original signed history");
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].acknowledgement.sequence, 0);
    assert_eq!(receipts[1].acknowledgement.sequence, 1);
    drop(runtime);
    let mut runtime = load(directory.path());
    let receipts = runtime
        .receive_repair_wires_with_receipts(
            "cluster-a",
            &identity,
            &[(second.wire_bytes().expect("wire"), 12)],
            &["local".to_owned()],
            "local",
        )
        .expect("retry original wire after restart");
    assert_eq!(receipts[0].acknowledgement.sequence, 1);
}

#[test]
fn invalid_original_wire_returns_no_receipts_and_rolls_back_the_page() {
    let directory = tempfile::tempdir().expect("directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let second = segment(
        &key,
        1,
        vec![record(b"second", false)],
        Some(first.segment_hash().expect("hash")),
    );
    let mut runtime = load(directory.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("baseline");
    let baseline = serde_json::to_value(runtime.snapshot_for_persistence()).expect("snapshot");
    assert!(
        runtime
            .receive_repair_wires_with_receipts(
                "cluster-a",
                &identity,
                &[(second.wire_bytes().expect("wire"), 12), (vec![0], 12)],
                &["local".to_owned()],
                "local",
            )
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(runtime.snapshot_for_persistence()).expect("snapshot"),
        baseline
    );
    drop(runtime);
    let runtime = load(directory.path());
    assert_eq!(
        serde_json::to_value(runtime.snapshot_for_persistence()).expect("snapshot"),
        baseline
    );
}

#[test]
fn original_import_preview_is_read_only_and_binds_wire_and_receiver_progress() {
    let directory = tempfile::tempdir().expect("directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let wire = first.wire_bytes().expect("wire");
    let mut runtime = load(directory.path());
    let before = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    let fingerprint = runtime
        .original_repair_fingerprint("cluster-a", &identity, &[(wire.clone(), 11)])
        .unwrap();
    assert_eq!(
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap(),
        before
    );
    assert!(
        runtime
            .original_repair_fingerprint("another-cluster", &identity, &[(wire.clone(), 11)])
            .is_err()
    );
    runtime
        .receive_wire("cluster-a", &identity, &wire, 11)
        .unwrap();
    assert_ne!(
        runtime
            .original_repair_fingerprint("cluster-a", &identity, &[(wire, 11)])
            .unwrap(),
        fingerprint
    );
}

#[test]
fn source_identity_change_preserves_old_outbox_then_starts_an_independent_chain() {
    let directory = tempfile::tempdir().unwrap();
    let collector_directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let old_identity = identity(&key);
    let new_identity = super::super::super::tests::identity_for(&key, "node-b");
    let mut source = load(directory.path());
    let old = source
        .queue_local_source_segments(
            "cluster-a",
            old_identity.clone(),
            &key,
            &[record(b"original", false)],
            100,
        )
        .unwrap();
    // Production legacy state has an epoch and outbox but no saved source node id.
    drop(source);
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    let raw: Vec<u8> = connection
        .query_row(
            "SELECT payload FROM history_snapshots WHERE key='repository_replica'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut legacy: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    legacy["local_source"]
        .as_object_mut()
        .unwrap()
        .remove("node_id");
    connection
        .execute(
            "UPDATE history_snapshots SET payload=?1 WHERE key='repository_replica'",
            [serde_json::to_vec(&legacy).unwrap()],
        )
        .unwrap();
    drop(connection);
    let mut source = load(directory.path());
    let before = source.original_source_page().unwrap();
    assert_eq!(before.len(), 1);
    assert!(
        source
            .queue_local_source_segments(
                "cluster-a",
                new_identity.clone(),
                &key,
                &[record(b"new", false)],
                101
            )
            .is_err()
    );
    assert_eq!(
        source.original_source_page().unwrap()[0].wire,
        before[0].wire
    );
    let mut collector = load(collector_directory.path());
    collector
        .receive_wire("cluster-a", &old_identity, &old[0].wire, 101)
        .unwrap();
    let ids = vec![hex::encode(sha2::Sha256::digest(&old[0].wire))];
    assert_eq!(collector.confirmed_original_segment_ids(&ids).unwrap(), ids);
    source
        .acknowledge_original_source_page(&old, 102, "direct")
        .unwrap();
    drop(source);
    let mut source = load(directory.path());
    assert!(source.original_source_page().unwrap().is_empty());
    let new = source
        .queue_local_source_segments(
            "cluster-a",
            new_identity.clone(),
            &key,
            &[record(b"new", false)],
            103,
        )
        .unwrap();
    let signed = crate::history_sync::SignedSegment::from_wire(&new[0].wire).unwrap();
    assert_eq!(signed.canonical().first_cursor().source_node_id(), "node-b");
    assert_eq!(signed.canonical().first_cursor().sequence(), 0);
    collector
        .receive_wire("cluster-a", &new_identity, &new[0].wire, 104)
        .unwrap();
}

#[test]
fn original_proof_does_not_confirm_a_source_only_outbox_or_missing_wire() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let mut source = load(directory.path());
    let page = source
        .queue_local_source_segments(
            "cluster-a",
            identity(&key),
            &key,
            &[record(b"not-at-collector", false)],
            100,
        )
        .unwrap();
    let id = hex::encode(sha2::Sha256::digest(&page[0].wire));
    assert!(
        source
            .confirmed_original_segment_ids(&[id, "0".repeat(64)])
            .unwrap()
            .is_empty()
    );
    assert_eq!(source.original_source_page().unwrap().len(), 1);
}

#[test]
fn unrelated_live_source_does_not_invalidate_original_import_preview() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(directory.path());
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    runtime
        .receive_wire("cluster-a", &identity, &first.wire_bytes().unwrap(), 11)
        .unwrap();
    let next = segment(
        &key,
        1,
        vec![record(b"next", false)],
        Some(first.segment_hash().unwrap()),
    );
    let wires = vec![(next.wire_bytes().unwrap(), 12)];
    let fingerprint = runtime
        .original_repair_fingerprint("cluster-a", &identity, &wires)
        .unwrap();
    let other_identity = super::super::super::tests::identity_for(&key, "node-b");
    let other = crate::history_sync::CanonicalSegment::new(
        "cluster-a",
        crate::history_sync::Cursor::new("node-b", 7, "runtime", 0).unwrap(),
        vec![record(b"live", false)],
        None,
        10,
        11,
    )
    .unwrap()
    .sign(&key)
    .unwrap();
    runtime
        .receive_wire(
            "cluster-a",
            &other_identity,
            &other.wire_bytes().unwrap(),
            12,
        )
        .unwrap();
    assert_eq!(
        runtime
            .original_repair_fingerprint("cluster-a", &identity, &wires)
            .unwrap(),
        fingerprint
    );
}

#[test]
fn historical_ingest_priority_requires_commit_and_expires_without_live_renewal() {
    use crate::state::history_repository::replica::RepositoryReplicaSegment;
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"live", false)], None);
    let second = segment(
        &key,
        1,
        vec![record(b"old", false)],
        Some(first.segment_hash().unwrap()),
    );
    let mut runtime = load(directory.path());
    let page = |wire| RepositoryReplicaSegment {
        identity: identity.clone(),
        wire,
    };
    runtime
        .receive_wire_batch_from_repository_with_gaps(
            "cluster-a",
            &[page(first.wire_bytes().unwrap())],
            &[],
            12,
            &["local".into()],
            "local",
        )
        .unwrap();
    assert!(!runtime.derived_maintenance_deferred());
    assert!(
        runtime
            .receive_wire_batch_from_repository_with_gaps(
                "cluster-a",
                &[page(second.wire_bytes().unwrap()), page(vec![0])],
                &[],
                3600,
                &["local".into()],
                "local"
            )
            .is_err()
    );
    assert!(!runtime.derived_maintenance_deferred());
    runtime
        .receive_wire_batch_from_repository_with_gaps(
            "cluster-a",
            &[page(second.wire_bytes().unwrap())],
            &[],
            3600,
            &["local".into()],
            "local",
        )
        .unwrap();
    assert!(runtime.derived_maintenance_deferred());
    runtime.historical_ingest_at =
        Some(std::time::Instant::now() - std::time::Duration::from_secs(31));
    assert!(!runtime.derived_maintenance_deferred());
    drop(runtime);
    assert!(!load(directory.path()).derived_maintenance_deferred());
}

#[test]
fn continuous_historical_ingest_cannot_renew_the_maintenance_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let wire = segment(&key, 0, vec![record(b"old", false)], None)
        .wire_bytes()
        .unwrap();
    let mut runtime = load(directory.path());
    let started = std::time::Instant::now() - std::time::Duration::from_secs(29);
    runtime.historical_ingest_at = Some(started);
    runtime.note_historical_ingest([wire.as_slice()], 3600);
    assert_eq!(runtime.historical_ingest_at, Some(started));
    assert!(runtime.derived_maintenance_deferred());
    runtime.historical_ingest_at =
        Some(std::time::Instant::now() - std::time::Duration::from_secs(31));
    runtime.note_historical_ingest([wire.as_slice()], 3600);
    runtime.prepare_for_replication(3600).unwrap();
    assert!(runtime.historical_ingest_at.is_none());
    runtime.note_historical_ingest([wire.as_slice()], 3600);
    assert!(runtime.derived_maintenance_deferred());
}
