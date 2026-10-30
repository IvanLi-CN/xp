use ed25519_dalek::SigningKey;

use super::super::*;
use super::{identity, identity_for, load, signing_key};
use crate::history_sync::{CanonicalSegment, SyncRecord};

fn node_record(key: &[u8], tombstone: bool) -> SyncRecord {
    SyncRecord::new(
        "node-a",
        "node-a",
        "traffic.v1",
        1,
        key.to_vec(),
        b"payload".to_vec(),
        tombstone,
    )
}

fn assert_foreign_tombstone_blocks_replay(deleted: &[u8], applied: &[u8]) {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let source_key = signing_key();
    let other_key = SigningKey::from_bytes(&[13; 32]);
    let original = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 0).unwrap(),
        vec![node_record(applied, false)],
        None,
        10,
        10,
    )
    .unwrap()
    .sign(&source_key)
    .unwrap();
    let tombstone = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-b", 8, "tombstone", 0).unwrap(),
        vec![node_record(deleted, true)],
        None,
        11,
        11,
    )
    .unwrap()
    .sign(&other_key)
    .unwrap();
    runtime
        .receive_wire(
            "cluster-a",
            &identity(&source_key),
            &original.wire_bytes().unwrap(),
            10,
        )
        .unwrap();
    runtime
        .receive_wire(
            "cluster-a",
            &identity_for(&other_key, "node-b"),
            &tombstone.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    assert!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 8)
            .unwrap()
            .is_empty()
    );
    drop(runtime);

    let mut runtime = load(directory.path());
    let replay = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 1).unwrap(),
        vec![node_record(applied, false)],
        Some(original.segment_hash().unwrap()),
        12,
        12,
    )
    .unwrap()
    .sign(&source_key)
    .unwrap();
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity(&source_key),
            &replay.wire_bytes().unwrap(),
            12
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::ResurrectionPrevented
        ))
    ));
    assert!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 8)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn foreign_node_prefix_tombstone_also_blocks_later_signed_original_stream() {
    assert_foreign_tombstone_blocks_replay(
        b"node-history:node:node-a:",
        b"node-history:node:node-a:daily:1",
    );
}

#[test]
fn foreign_exact_tombstone_also_blocks_later_signed_original_stream() {
    assert_foreign_tombstone_blocks_replay(
        b"node-history:node:node-a:daily:1",
        b"node-history:node:node-a:daily:1",
    );
}

#[test]
fn exact_tombstone_does_not_block_a_different_backfill_stream() {
    let marker_cursor = ReplicaCursor::new("node-b", 8, "tombstone", 0).unwrap();
    let other_cursor = ReplicaCursor::new("node-a", 7, "traffic-backfill-node-a", 0).unwrap();
    let make = |cursor: &ReplicaCursor| {
        ReplicaRecord::new(
            cursor,
            "node-a",
            "node-a",
            "traffic.v1",
            1,
            b"node-history:node:node-a:daily:1".to_vec(),
            b"payload".to_vec(),
        )
        .unwrap()
        .key()
    };
    let mut ledger = TombstoneLedger::new(100);
    ledger
        .tombstone(make(&marker_cursor), 10, ["local"])
        .unwrap();
    assert!(ledger.allows(&make(&other_cursor)));
}

#[test]
fn same_transaction_tombstone_dominates_earlier_insert_and_preserves_other_subject() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let removed = RepositoryHistoryRecordRow {
        source_node_id: "node-a".into(),
        source_epoch: 7,
        stream: "traffic".into(),
        sequence: 1,
        subject_node_id: "node-a".into(),
        observer_node_id: "node-a".into(),
        schema_id: "traffic.v1".into(),
        schema_version: 1,
        record_key: b"node-history:node:node-a:daily:1".to_vec(),
        tombstone: false,
        observed_start_unix_seconds: 10,
        observed_end_unix_seconds: 10,
        received_at_unix_seconds: 10,
        aggregate_complete: None,
        aggregate_start_unix_seconds: None,
        aggregate_end_unix_seconds: None,
        payload: b"retained".to_vec(),
    };
    let retained = RepositoryHistoryRecordRow {
        subject_node_id: "node-c".into(),
        observer_node_id: "node-c".into(),
        sequence: 2,
        ..removed.clone()
    };
    let tombstone = RepositoryHistoryTombstone {
        source_node_id: "node-b".into(),
        source_epoch: 8,
        stream: "traffic".into(),
        source_stream: None,
        subject_node_id: "node-a".into(),
        observer_node_id: "node-a".into(),
        schema_id: "traffic.v1".into(),
        schema_version: 1,
        record_key: b"node-history:node:node-a:".to_vec(),
        prefix: true,
    };
    storage
        .commit_repository_replica_mutation(
            RepositoryReplicaMutation {
                target_repository_id: "local".into(),
                records: vec![removed, retained],
                tombstones: vec![tombstone],
                ..RepositoryReplicaMutation::default()
            },
            b"{}",
        )
        .unwrap();
    drop(storage);
    let storage = HistoryStorage::open(directory.path());
    let records = storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].subject_node_id, "node-c");
    assert_eq!(storage.repository_history_record_count().unwrap(), 1);
}

#[test]
fn historical_repair_preserves_wire_without_resurrecting_known_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let source_key = signing_key();
    let other_key = SigningKey::from_bytes(&[13; 32]);
    let key = b"node-history:node:node-a:daily:1";
    let deletion = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-b", 8, "tombstone", 0).unwrap(),
        vec![node_record(b"node-history:node:node-a:", true)],
        None,
        11,
        11,
    )
    .unwrap()
    .sign(&other_key)
    .unwrap();
    runtime
        .receive_wire(
            "cluster-a",
            &identity_for(&other_key, "node-b"),
            &deletion.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    let historical = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 0).unwrap(),
        vec![node_record(key, false)],
        None,
        10,
        10,
    )
    .unwrap()
    .sign(&source_key)
    .unwrap();
    let wire = historical.wire_bytes().unwrap();
    assert!(matches!(
        runtime.receive_wire("cluster-a", &identity(&source_key), &wire, 12),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::ResurrectionPrevented
        ))
    ));
    runtime
        .receive_repair_wire_from_repository_with_gaps(
            "cluster-a",
            &identity(&source_key),
            &wire,
            &[],
            12,
            &["local".into()],
            "local",
        )
        .unwrap();
    drop(runtime);
    let mut runtime = load(directory.path());
    assert!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 8)
            .unwrap()
            .is_empty()
    );
    let id = hex::encode(sha2::Sha256::digest(&wire));
    assert_eq!(runtime.repair_batch(&[id]).unwrap().segments[0].wire, wire);
    runtime
        .receive_repair_wire_from_repository_with_gaps(
            "cluster-a",
            &identity(&source_key),
            &wire,
            &[],
            13,
            &["local".into()],
            "local",
        )
        .unwrap();
}

#[test]
fn retained_history_anchor_does_not_resurrect_known_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let source_key = signing_key();
    let other_key = SigningKey::from_bytes(&[13; 32]);
    let deletion = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-b", 8, "tombstone", 0).unwrap(),
        vec![node_record(b"node-history:node:node-a:", true)],
        None,
        11,
        11,
    )
    .unwrap()
    .sign(&other_key)
    .unwrap();
    runtime
        .receive_wire(
            "cluster-a",
            &identity_for(&other_key, "node-b"),
            &deletion.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    let historical = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 5).unwrap(),
        vec![node_record(b"node-history:node:node-a:daily:1", false)],
        Some([7; 32]),
        10,
        10,
    )
    .unwrap()
    .sign(&source_key)
    .unwrap();
    runtime
        .receive_initial_backfill_wire_from_repository(
            "cluster-a",
            &identity(&source_key),
            &historical.wire_bytes().unwrap(),
            12,
            &["local".into()],
            "local",
        )
        .unwrap();
    drop(runtime);
    let runtime = load(directory.path());
    assert!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 8)
            .unwrap()
            .is_empty()
    );
    let id = hex::encode(sha2::Sha256::digest(historical.wire_bytes().unwrap()));
    assert_eq!(
        runtime.repair_batch(&[id]).unwrap().segments[0].wire,
        historical.wire_bytes().unwrap()
    );
}

#[test]
fn historical_repair_applies_same_wire_deletion_before_matching_records() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let key = signing_key();
    let deleted = b"node-history:node:node-a:daily:1";
    let retained = b"node-history:node:node-a:daily:2";
    let signed = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 0).unwrap(),
        vec![
            node_record(deleted, true),
            node_record(deleted, false),
            node_record(retained, false),
        ],
        None,
        10,
        10,
    )
    .unwrap()
    .sign(&key)
    .unwrap();
    let wire = signed.wire_bytes().unwrap();
    assert!(matches!(
        runtime.receive_wire("cluster-a", &identity(&key), &wire, 11),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::ResurrectionPrevented
        ))
    ));
    runtime
        .receive_repair_wire_from_repository_with_gaps(
            "cluster-a",
            &identity(&key),
            &wire,
            &[],
            11,
            &["local".into()],
            "local",
        )
        .unwrap();
    drop(runtime);
    let runtime = load(directory.path());
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].record_key, retained);
    let id = hex::encode(sha2::Sha256::digest(&wire));
    assert_eq!(runtime.repair_batch(&[id]).unwrap().segments[0].wire, wire);
}

#[test]
fn historical_mixed_tombstone_stream_deletes_only_matching_source_origin() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let source_key = signing_key();
    let other_key = SigningKey::from_bytes(&[13; 32]);
    let deleted = b"node-history:node:node-a:daily:1";
    let make =
        |node: &str, epoch: u64, seq: u64, records: Vec<SyncRecord>, previous, key: &SigningKey| {
            CanonicalSegment::new(
                "cluster-a",
                Cursor::new(node, epoch, "tombstone", seq).unwrap(),
                records,
                previous,
                10,
                10,
            )
            .unwrap()
            .sign(key)
            .unwrap()
        };
    let prior = make(
        "node-a",
        7,
        0,
        vec![node_record(deleted, false)],
        None,
        &source_key,
    );
    let foreign = make(
        "node-b",
        8,
        0,
        vec![node_record(deleted, false)],
        None,
        &other_key,
    );
    runtime
        .receive_wire(
            "cluster-a",
            &identity(&source_key),
            &prior.wire_bytes().unwrap(),
            10,
        )
        .unwrap();
    runtime
        .receive_wire(
            "cluster-a",
            &identity_for(&other_key, "node-b"),
            &foreign.wire_bytes().unwrap(),
            10,
        )
        .unwrap();
    let mixed = make(
        "node-a",
        7,
        1,
        vec![
            node_record(deleted, true),
            node_record(deleted, false),
            node_record(b"other", false),
        ],
        Some(prior.segment_hash().unwrap()),
        &source_key,
    );
    let wire = mixed.wire_bytes().unwrap();
    runtime
        .receive_repair_wire_from_repository_with_gaps(
            "cluster-a",
            &identity(&source_key),
            &wire,
            &[],
            11,
            &["local".into()],
            "local",
        )
        .unwrap();
    drop(runtime);
    let runtime = load(directory.path());
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .any(|row| row.source_node_id == "node-b" && row.record_key == deleted)
    );
    assert!(
        rows.iter()
            .any(|row| row.source_node_id == "node-a" && row.record_key == b"other")
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.source_node_id == "node-a" && row.record_key == deleted)
    );
}
