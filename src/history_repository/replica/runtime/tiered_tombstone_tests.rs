use super::super::tests::{identity_for, load, signing_key};
use super::*;
use crate::history_sync::CanonicalSegment;

const DELETED_PREFIX: &[u8] = b"node-history:node:node-a:";
const DELETED_KEY: &[u8] = b"node-history:node:node-a:daily:1";

fn historical_record(key: &[u8], sequence: u64) -> RepositoryTieredBackfillRecord {
    RepositoryTieredBackfillRecord {
        observed_at_unix_seconds: 10,
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "traffic".to_owned(),
        sequence,
        subject_node_id: "node-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "traffic.v1".to_owned(),
        schema_version: 1,
        record_key: key.to_vec(),
        payload: b"original historical payload".to_vec(),
        tombstone: false,
    }
}

fn seed_deletion(runtime: &mut RepositoryReplicaRuntime, key: &[u8]) {
    let signing_key = signing_key();
    let marker = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-b", 8, "tombstone", 0).unwrap(),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "traffic.v1",
            1,
            key.to_vec(),
            b"deleted".to_vec(),
            true,
        )],
        None,
        11,
        11,
    )
    .unwrap()
    .sign(&signing_key)
    .unwrap();
    runtime
        .receive_wire(
            "cluster-a",
            &identity_for(&signing_key, "node-b"),
            &marker.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
}

fn import(
    runtime: &mut RepositoryReplicaRuntime,
    records: Vec<RepositoryTieredBackfillRecord>,
    now: u64,
) -> Result<(), RepositoryRuntimeError> {
    runtime.import_tiered_backfill_records(records, now, &["local".to_owned()], "local")
}

fn stored_keys(runtime: &RepositoryReplicaRuntime) -> Vec<Vec<u8>> {
    runtime
        .storage
        .repository_history_records(None, None, None, 0, 128)
        .unwrap()
        .into_iter()
        .map(|record| record.record_key)
        .collect()
}

fn transaction_state(runtime: &RepositoryReplicaRuntime) -> serde_json::Value {
    serde_json::to_value((
        &runtime.snapshot.receiver,
        runtime.tombstones.checkpoint(),
        &runtime.snapshot.source_last_received_unix_seconds,
        &runtime.snapshot.initial_peer_backfills,
        &runtime.snapshot.gaps,
        &runtime.snapshot.local_source,
    ))
    .unwrap()
}

fn seed_page_checkpoint(runtime: &mut RepositoryReplicaRuntime) {
    runtime
        .update_initial_peer_backfill_checkpoint(
            "ready-peer",
            Some("page-before-import".to_owned()),
            BTreeMap::new(),
            false,
            false,
        )
        .unwrap();
}

#[test]
fn persisted_foreign_deletion_suppresses_only_matching_history_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    seed_deletion(&mut runtime, DELETED_PREFIX);
    seed_page_checkpoint(&mut runtime);
    drop(runtime);

    let mut runtime = load(directory.path());
    let checkpoint = runtime.initial_peer_backfill_checkpoint("ready-peer");
    let ledger = serde_json::to_value(runtime.tombstones.checkpoint()).unwrap();
    let records = vec![
        historical_record(DELETED_KEY, 2262),
        historical_record(b"unrelated", 2263),
    ];
    import(&mut runtime, records.clone(), 12).expect("deleted history must not block the page");
    assert_eq!(stored_keys(&runtime), vec![b"unrelated".to_vec()]);
    assert_eq!(
        runtime.snapshot.source_last_received_unix_seconds["node-a"],
        12
    );
    assert_eq!(
        runtime.initial_peer_backfill_checkpoint("ready-peer"),
        checkpoint
    );
    assert_eq!(
        serde_json::to_value(runtime.tombstones.checkpoint()).unwrap(),
        ledger
    );
    drop(runtime);

    // Crash before the worker's separate page checkpoint: replay must stay idempotent.
    let mut runtime = load(directory.path());
    assert_eq!(
        runtime.initial_peer_backfill_checkpoint("ready-peer"),
        checkpoint
    );
    import(&mut runtime, records, 13).unwrap();
    runtime
        .update_initial_peer_backfill_checkpoint("ready-peer", None, BTreeMap::new(), true, true)
        .unwrap();
    drop(runtime);
    let runtime = load(directory.path());
    assert_eq!(stored_keys(&runtime), vec![b"unrelated".to_vec()]);
    assert!(
        runtime
            .initial_peer_backfill_checkpoint("ready-peer")
            .unwrap()
            .completed
    );
    assert_eq!(
        runtime.snapshot.source_last_received_unix_seconds["node-a"],
        13
    );
    assert_eq!(
        serde_json::to_value(runtime.tombstones.checkpoint()).unwrap(),
        ledger
    );
}

#[test]
fn prefix_and_exact_deletions_preserve_every_nonmatching_identity_dimension() {
    for deletion in [DELETED_PREFIX, DELETED_KEY] {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime = load(directory.path());
        seed_deletion(&mut runtime, deletion);
        drop(runtime);
        let mut runtime = load(directory.path());
        let deleted = historical_record(DELETED_KEY, 2262);
        let mut kept = vec![deleted.clone(); 5];
        kept[0].subject_node_id = "other-subject".to_owned();
        kept[1].observer_node_id = "other-observer".to_owned();
        kept[2].schema_id = "runtime.v1".to_owned();
        kept[3].schema_version = 2;
        kept[4].record_key = b"node-history:node:node-ab:daily:1".to_vec();
        let mut page = vec![deleted];
        for (index, record) in kept.iter_mut().enumerate() {
            record.sequence = 2263 + index as u64;
            record.payload = format!("retained payload {index}").into_bytes();
            page.push(record.clone());
        }
        import(&mut runtime, page, 12).unwrap();
        drop(runtime);
        let runtime = load(directory.path());
        let rows = runtime
            .storage
            .repository_history_records(None, None, None, 0, 128)
            .unwrap();
        assert_eq!(rows.len(), kept.len());
        for expected in kept {
            let row = rows
                .iter()
                .find(|row| row.sequence == expected.sequence)
                .unwrap();
            assert_eq!(row.source_node_id, expected.source_node_id);
            assert_eq!(row.source_epoch, expected.source_epoch);
            assert_eq!(row.stream, expected.stream);
            assert_eq!(row.subject_node_id, expected.subject_node_id);
            assert_eq!(row.observer_node_id, expected.observer_node_id);
            assert_eq!(row.schema_id, expected.schema_id);
            assert_eq!(row.schema_version, expected.schema_version);
            assert_eq!(row.record_key, expected.record_key);
            let stored: StoredRecord = serde_json::from_slice(&row.payload).unwrap();
            assert_eq!(stored.payload, expected.payload);
            assert_eq!(row.observed_start_unix_seconds, 10);
        }
    }
}

#[test]
fn exact_deletion_suppresses_its_target_streams_without_widening_to_other_backfills() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    seed_deletion(&mut runtime, DELETED_KEY);
    let original = historical_record(DELETED_KEY, 2262);
    let mut targeted_backfill = original.clone();
    targeted_backfill.stream = "traffic-backfill-node-b".to_owned();
    targeted_backfill.source_node_id = "node-c".to_owned();
    targeted_backfill.source_epoch = 99;
    let mut other_backfill = original.clone();
    other_backfill.stream = "traffic-backfill-node-a".to_owned();
    import(
        &mut runtime,
        vec![original, targeted_backfill, other_backfill],
        12,
    )
    .unwrap();
    drop(runtime);
    let runtime = load(directory.path());
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 128)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].stream, "traffic-backfill-node-a");
    assert_eq!(
        runtime.snapshot.source_last_received_unix_seconds["node-c"],
        12
    );
}

#[test]
fn entirely_deleted_page_commits_receipt_progress_and_stays_deleted_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    seed_deletion(&mut runtime, DELETED_PREFIX);
    let state = serde_json::to_value(runtime.tombstones.checkpoint()).unwrap();
    let mut record = historical_record(DELETED_KEY, 2262);
    record.source_node_id = "old-source".to_owned();
    record.source_epoch = 999;
    record.stream = "traffic-backfill-another-source".to_owned();
    import(&mut runtime, vec![record.clone()], 12).unwrap();
    drop(runtime);
    let mut runtime = load(directory.path());
    assert!(stored_keys(&runtime).is_empty());
    assert_eq!(
        runtime.snapshot.source_last_received_unix_seconds["old-source"],
        12
    );
    import(&mut runtime, vec![record], 13).unwrap();
    drop(runtime);
    let runtime = load(directory.path());
    assert!(stored_keys(&runtime).is_empty());
    assert_eq!(
        runtime.snapshot.source_last_received_unix_seconds["old-source"],
        13
    );
    assert_eq!(
        serde_json::to_value(runtime.tombstones.checkpoint()).unwrap(),
        state
    );
}

#[test]
fn page_tombstone_deletes_existing_and_same_page_history_in_either_order() {
    for tombstone_first in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime = load(directory.path());
        let old = historical_record(DELETED_KEY, 1);
        import(&mut runtime, vec![old], 10).unwrap();
        let mut marker = historical_record(DELETED_PREFIX, 0);
        marker.source_node_id = "node-b".to_owned();
        marker.source_epoch = 8;
        marker.stream = "tombstone".to_owned();
        marker.tombstone = true;
        let mut page = vec![historical_record(DELETED_KEY, 2262), marker];
        if tombstone_first {
            page.reverse();
        }
        page.push(historical_record(b"retained", 2263));
        import(&mut runtime, page, 12).unwrap();
        drop(runtime);
        let mut runtime = load(directory.path());
        assert_eq!(stored_keys(&runtime), vec![b"retained".to_vec()]);
        import(&mut runtime, vec![historical_record(DELETED_KEY, 2262)], 13).unwrap();
        assert_eq!(stored_keys(&runtime), vec![b"retained".to_vec()]);
        let ledger = serde_json::to_value(runtime.tombstones.checkpoint()).unwrap();
        assert_eq!(ledger["entries"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn malformed_deleted_row_rolls_back_prior_rows_deletions_and_checkpoints() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    seed_deletion(&mut runtime, DELETED_PREFIX);
    import(&mut runtime, vec![historical_record(b"existing", 1)], 11).unwrap();
    seed_page_checkpoint(&mut runtime);
    let before = transaction_state(&runtime);
    let durable = runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap();
    let mut marker = historical_record(b"existing", 0);
    marker.source_node_id = "node-c".to_owned();
    marker.stream = "tombstone".to_owned();
    marker.tombstone = true;
    let mut invalid = historical_record(DELETED_KEY, 2263);
    invalid.stream.clear();
    let error = import(
        &mut runtime,
        vec![
            historical_record(DELETED_KEY, 2262),
            marker,
            historical_record(b"uncommitted", 2264),
            invalid,
        ],
        12,
    )
    .expect_err("deletion suppression must not bypass row identity validation");
    assert!(matches!(error, RepositoryRuntimeError::Replica(_)));
    assert_eq!(transaction_state(&runtime), before);
    assert_eq!(
        runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        durable
    );
    assert_eq!(stored_keys(&runtime), vec![b"existing".to_vec()]);
    drop(runtime);
    let runtime = load(directory.path());
    assert_eq!(transaction_state(&runtime), before);
    assert_eq!(stored_keys(&runtime), vec![b"existing".to_vec()]);
}

#[test]
fn sqlite_commit_failure_rolls_back_suppression_rows_deletions_and_checkpoints() {
    for entirely_deleted in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime = load(directory.path());
        seed_deletion(&mut runtime, DELETED_PREFIX);
        import(&mut runtime, vec![historical_record(b"existing", 1)], 11).unwrap();
        seed_page_checkpoint(&mut runtime);
        let before = transaction_state(&runtime);
        let checkpoint = runtime.initial_peer_backfill_checkpoint("ready-peer");
        let durable = runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap();
        let mut page = vec![historical_record(DELETED_KEY, 2262)];
        if !entirely_deleted {
            let mut marker = historical_record(b"existing", 0);
            marker.source_node_id = "node-c".to_owned();
            marker.stream = "tombstone".to_owned();
            marker.tombstone = true;
            page.extend([marker, historical_record(b"uncommitted", 2263)]);
        }
        runtime.storage.set_query_only_for_test(true).unwrap();
        let error = import(&mut runtime, page.clone(), 12)
            .expect_err("even an entirely suppressed page must commit its checkpoint");
        assert!(matches!(error, RepositoryRuntimeError::Storage(_)));
        assert_eq!(transaction_state(&runtime), before);
        assert_eq!(
            runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
            durable
        );
        assert_eq!(stored_keys(&runtime), vec![b"existing".to_vec()]);
        runtime.storage.set_query_only_for_test(false).unwrap();
        drop(runtime);
        let mut runtime = load(directory.path());
        assert_eq!(transaction_state(&runtime), before);
        import(&mut runtime, page, 13).unwrap();
        drop(runtime);
        let runtime = load(directory.path());
        let expected: &[u8] = if entirely_deleted {
            b"existing"
        } else {
            b"uncommitted"
        };
        assert_eq!(stored_keys(&runtime), vec![expected.to_vec()]);
        assert_eq!(
            runtime.snapshot.source_last_received_unix_seconds["node-a"],
            13
        );
        assert_eq!(
            runtime.initial_peer_backfill_checkpoint("ready-peer"),
            checkpoint
        );
    }
}

#[test]
fn checkpoint_failure_rolls_back_rows_and_deletions_already_executed_in_the_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    seed_deletion(&mut runtime, DELETED_PREFIX);
    import(&mut runtime, vec![historical_record(b"existing", 1)], 11).unwrap();
    seed_page_checkpoint(&mut runtime);
    let before = transaction_state(&runtime);
    let durable = runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap();
    let count = runtime.storage.repository_history_record_count().unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_tiered_checkpoint BEFORE INSERT ON history_snapshots
             WHEN NEW.key = 'repository_replica'
             BEGIN SELECT RAISE(ABORT, 'injected tiered checkpoint failure'); END;",
        )
        .unwrap();
    let mut marker = historical_record(b"existing", 0);
    marker.source_node_id = "node-c".to_owned();
    marker.stream = "tombstone".to_owned();
    marker.tombstone = true;
    let page = vec![
        historical_record(DELETED_KEY, 2262),
        marker,
        historical_record(b"retained", 2263),
    ];
    let error = import(&mut runtime, page.clone(), 12)
        .expect_err("checkpoint must fail after row upserts and tombstone deletion");
    assert!(matches!(error, RepositoryRuntimeError::Storage(_)));
    assert!(
        error
            .to_string()
            .contains("injected tiered checkpoint failure")
    );
    assert_eq!(transaction_state(&runtime), before);
    assert_eq!(
        runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        durable
    );
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        count
    );
    assert_eq!(stored_keys(&runtime), vec![b"existing".to_vec()]);
    connection
        .execute_batch("DROP TRIGGER fail_tiered_checkpoint")
        .unwrap();
    drop(connection);
    drop(runtime);
    let mut runtime = load(directory.path());
    assert_eq!(transaction_state(&runtime), before);
    assert_eq!(stored_keys(&runtime), vec![b"existing".to_vec()]);
    import(&mut runtime, page, 13).unwrap();
    drop(runtime);
    let runtime = load(directory.path());
    assert_eq!(stored_keys(&runtime), vec![b"retained".to_vec()]);
    assert_eq!(
        runtime.snapshot.source_last_received_unix_seconds["node-a"],
        13
    );
}

#[test]
fn historical_suppression_does_not_relax_ordinary_live_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    seed_deletion(&mut runtime, DELETED_PREFIX);
    import(&mut runtime, vec![historical_record(DELETED_KEY, 2262)], 12).unwrap();
    drop(runtime);
    let mut runtime = load(directory.path());
    let before = transaction_state(&runtime);
    let key = signing_key();
    let segment = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 0).unwrap(),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "traffic.v1",
            1,
            DELETED_KEY.to_vec(),
            b"live replay".to_vec(),
            false,
        )],
        None,
        13,
        13,
    )
    .unwrap()
    .sign(&key)
    .unwrap();
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity_for(&key, "node-a"),
            &segment.wire_bytes().unwrap(),
            13,
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::ResurrectionPrevented
        ))
    ));
    assert!(stored_keys(&runtime).is_empty());
    assert_eq!(transaction_state(&runtime), before);
}
