use super::super::StoredRecord;
use super::*;
use sha2::Sha256;

#[test]
fn syncing_tombstone_receipt_is_deferred_until_repository_is_ready() {
    let source_dir = tempfile::tempdir().expect("source directory");
    let collector_dir = tempfile::tempdir().expect("collector directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let tombstone = SyncRecord::new(
        "node-a",
        "node-a",
        "traffic.v1",
        1,
        b"node-history:node:node-a:".to_vec(),
        b"deleted".to_vec(),
        true,
    );
    let mut source = load(source_dir.path());
    let queued = source
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            source_identity.clone(),
            &key,
            vec![tombstone],
            100,
            &["repo-a".to_owned(), "repo-b".to_owned()],
        )
        .expect("queue tombstone");
    let mut collector = load(collector_dir.path());
    let syncing_receipt = collector
        .receive_wire_from_repository(
            "cluster-a",
            &source_identity,
            &queued[0].wire,
            100,
            &["repo-a".to_owned()],
            "repo-b",
        )
        .expect("syncing collector accepts tombstone");
    assert!(syncing_receipt.tombstone_acknowledgements().is_empty());

    let ready_receipt = collector
        .receive_wire_from_repository(
            "cluster-a",
            &source_identity,
            &queued[0].wire,
            101,
            &["repo-a".to_owned(), "repo-b".to_owned()],
            "repo-b",
        )
        .expect("ready collector replays tombstone acknowledgement");
    assert_eq!(ready_receipt.tombstone_acknowledgements().len(), 1);
}

#[test]
fn source_backlog_does_not_become_a_permanent_gap_when_delivery_is_delayed() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    for sequence in 0..9_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue delayed source segment");
    }

    assert!(runtime.local_source_backpressure_gaps("node-a").is_empty());
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 9);
    assert_eq!(runtime.local_source_next_sequence("runtime"), Some(9));

    let front = runtime
        .local_source_pending_segments()
        .into_iter()
        .next()
        .expect("front source segment");
    runtime
        .acknowledge_local_source_segment(&front.wire)
        .expect("acknowledge source segment");
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 8);
    drop(runtime);
    let restored = load(temporary.path());
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 8);
    assert_eq!(restored.local_source_next_sequence("runtime"), Some(9));
    assert_eq!(restored.local_source_pending_segments().len(), 1);
}

#[test]
fn source_journal_summary_keeps_only_the_oldest_wire() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    for sequence in 0..3_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    vec![u8::try_from(sequence).unwrap(); 4 * 1024],
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }

    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let rows = storage.source_delivery_journal().expect("read journal");
    let expected_bytes = rows.iter().map(|row| row.wire.len() as u64).sum::<u64>();
    let expected_oldest_id = rows.first().expect("oldest row").id.clone();

    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize journal");

    assert_eq!(summary.pending_segments, rows.len());
    assert_eq!(summary.pending_bytes, expected_bytes);
    assert_eq!(summary.oldest.expect("oldest row").id, expected_oldest_id);
}

#[test]
fn source_delivery_journal_state_tracks_upserts_and_ack_path() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    for sequence in 0..3_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    vec![u8::try_from(sequence + 1).unwrap(); 1024],
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }

    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let rows = storage.source_delivery_journal().expect("read journal");
    let expected_bytes = rows.iter().map(|row| row.wire.len() as u64).sum::<u64>();
    storage
        .append_source_delivery_journal(&rows)
        .expect("upsert journal rows");
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize upserted journal");
    assert_eq!(summary.pending_segments, rows.len());
    assert_eq!(summary.pending_bytes, expected_bytes);

    runtime
        .acknowledge_local_source_segment_via(&rows[0].wire, 200, "direct")
        .expect("acknowledge journal row");
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize acknowledged journal");
    assert_eq!(summary.pending_segments, rows.len() - 1);
    assert_eq!(
        summary.pending_bytes,
        expected_bytes - rows[0].wire.len() as u64
    );
    assert_eq!(summary.last_acknowledged_at, Some(200));
    assert_eq!(summary.last_delivery_path.as_deref(), Some("direct"));

    storage
        .acknowledge_source_delivery_journal(
            &[hex::encode(Sha256::digest(&rows[0].wire))],
            Some(300),
            Some("dynamic_relay"),
        )
        .expect("repeat acknowledgement is idempotent");
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize repeated acknowledgement");
    assert_eq!(summary.last_acknowledged_at, Some(200));
    assert_eq!(summary.last_delivery_path.as_deref(), Some("direct"));
}

#[test]
fn source_delivery_journal_max_epoch_reads_state_without_decoding_wire() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    drop(storage);
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    connection
        .execute(
            "INSERT INTO source_delivery_journal
                 (id, stream, closed_at, identity, wire, created_at,
                  source_node_id, source_epoch, first_sequence)
             VALUES ('invalid-wire', 'runtime', 100, X'00', X'00', 100, 'node-a', 7, 0)",
            [],
        )
        .expect("insert opaque journal row");
    connection
        .execute(
            "UPDATE source_delivery_journal_state
             SET epoch_high_water = 7
             WHERE singleton = 1",
            [],
        )
        .expect("set epoch high-water");

    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    assert_eq!(
        storage
            .source_delivery_journal_max_epoch()
            .expect("read epoch high-water"),
        Some(7)
    );
}

#[test]
fn source_delivery_journal_backlog_work_is_bounded() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let source_identity = identity();
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    drop(storage);
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    let identity = serde_json::to_vec(&source_identity).expect("serialize source identity");
    let wire = vec![0_u8; 7 * 1024];
    let transaction = connection
        .unchecked_transaction()
        .expect("begin backlog transaction");
    for sequence in 0..20_000_i64 {
        transaction
            .execute(
                "INSERT INTO source_delivery_journal
                     (id, stream, closed_at, identity, wire, created_at,
                      source_node_id, source_epoch, first_sequence)
                 VALUES (?1, ?2, 100, ?3, ?4, 100, 'node-a', 1, ?5)",
                rusqlite::params![
                    format!("segment-{sequence}"),
                    if sequence % 100 == 0 {
                        "tombstone"
                    } else {
                        "runtime"
                    },
                    &identity,
                    &wire,
                    sequence,
                ],
            )
            .expect("insert backlog row");
    }
    transaction.commit().expect("commit backlog transaction");

    let plan = connection
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT id, stream, closed_at, identity, wire
             FROM source_delivery_journal
             ORDER BY (stream = 'tombstone') DESC, source_node_id, source_epoch,
                      stream, first_sequence, created_at, id
             LIMIT 256",
        )
        .expect("prepare journal page plan")
        .query_map([], |row| row.get::<_, String>(3))
        .expect("read journal page plan")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("collect journal page plan")
        .join(" ");

    assert!(
        plan.contains("source_delivery_journal_delivery_order"),
        "journal page must use the delivery-order index: {plan}"
    );
    assert!(
        !plan.to_ascii_uppercase().contains("TEMP B-TREE"),
        "journal page must not sort the full backlog: {plan}"
    );
}

#[test]
fn legacy_source_journal_order_repair_crosses_page_boundary() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    for sequence in 0..257_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }
    drop(runtime);

    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    connection
        .execute(
            "UPDATE source_delivery_journal
             SET source_node_id = '', source_epoch = 0, first_sequence = 0",
            [],
        )
        .expect("simulate legacy journal order columns");
    drop(connection);

    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let summary = storage
        .source_delivery_journal_summary()
        .expect("repair and summarize legacy journal");
    assert_eq!(summary.pending_segments, 257);
    drop(storage);

    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("reopen history database");
    let remaining = connection
        .query_row(
            "SELECT COUNT(*) FROM source_delivery_journal WHERE source_node_id = ''",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("count unrepaired journal rows");
    assert_eq!(remaining, 0);
}

#[test]
fn source_journal_epoch_recovery_scans_beyond_the_replay_page() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    for sequence in 0..256_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }
    let high_epoch = i64::MAX as u64 - 1;
    let high_epoch_segment = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", high_epoch, "runtime", 0).expect("cursor"),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "runtime.v1",
            1,
            b"high-epoch".to_vec(),
            b"sample".to_vec(),
            false,
        )],
        None,
        10_000,
        10_001,
    )
    .expect("high epoch segment")
    .sign(&key)
    .expect("high epoch signature");
    let high_epoch_wire = high_epoch_segment.wire_bytes().expect("high epoch wire");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    storage
        .append_source_delivery_journal(&[
            crate::state::history_storage::SourceDeliveryJournalRow {
                id: hex::encode(Sha256::digest(&high_epoch_wire)),
                stream: "runtime".to_owned(),
                closed_at_unix_seconds: 10_001,
                identity: source_identity,
                wire: high_epoch_wire,
            },
        ])
        .expect("append high epoch journal row");
    storage
        .write(crate::state::history_storage::REPOSITORY_REPLICA_KEY, b"{}")
        .expect("simulate lost control snapshot");

    let restored = load(temporary.path());
    assert_eq!(restored.snapshot.local_source.epoch(), high_epoch + 1);
}

#[test]
fn source_journal_epoch_recovery_rejects_exhausted_epoch() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let segment = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", i64::MAX as u64, "runtime", 0).expect("cursor"),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "runtime.v1",
            1,
            b"exhausted-epoch".to_vec(),
            b"sample".to_vec(),
            false,
        )],
        None,
        10,
        11,
    )
    .expect("segment")
    .sign(&key)
    .expect("signature");
    let wire = segment.wire_bytes().expect("wire");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    storage
        .append_source_delivery_journal(&[
            crate::state::history_storage::SourceDeliveryJournalRow {
                id: hex::encode(Sha256::digest(&wire)),
                stream: "runtime".to_owned(),
                closed_at_unix_seconds: 11,
                identity: source_identity,
                wire,
            },
        ])
        .expect("append journal row");
    storage
        .write(crate::state::history_storage::REPOSITORY_REPLICA_KEY, b"{}")
        .expect("simulate lost control snapshot");

    assert!(RepositoryReplicaRuntime::load(storage).is_err());
}

#[test]
fn source_journal_rejects_out_of_order_acknowledgement() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    for sequence in 0..2_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let journal = storage.source_delivery_journal().expect("read journal");
    assert!(
        runtime
            .acknowledge_local_source_segment(&journal[1].wire)
            .is_err()
    );
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 2);
    assert_eq!(runtime.local_source_pending_segments().len(), 1);
}

#[test]
fn source_journal_ack_write_failure_preserves_pending_segment_for_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    let queued = runtime
        .queue_local_source_segment(
            "cluster-a",
            source_identity,
            &key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:retry".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            1,
        )
        .expect("queue source segment")
        .expect("queued source segment");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    storage
        .set_query_only_for_test(true)
        .expect("enable SQLite write failure");

    assert!(
        runtime
            .acknowledge_local_source_segment(&queued.wire)
            .is_err()
    );
    assert_eq!(runtime.local_source_pending_segments().len(), 1);
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 1);

    storage
        .set_query_only_for_test(false)
        .expect("disable SQLite write failure");
    runtime
        .acknowledge_local_source_segment(&queued.wire)
        .expect("retry source acknowledgement");
    assert!(runtime.local_source_pending_segments().is_empty());
    assert!(storage.source_delivery_journal().unwrap().is_empty());
}

#[test]
fn backfill_ack_write_failure_preserves_inflight_page_for_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    let page = runtime
        .queue_local_history_backfill_batches(
            "cluster-a",
            source_identity,
            &key,
            Some("history-page-1".to_owned()),
            false,
            vec![(
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    b"runtime:backfill-retry".to_vec(),
                    b"sample".to_vec(),
                    false,
                )],
                1,
            )],
        )
        .expect("queue backfill page");
    assert_eq!(page.len(), 1);
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    storage
        .set_query_only_for_test(true)
        .expect("enable SQLite write failure");

    assert!(
        runtime
            .acknowledge_local_source_segment_and_checkpoint_backfill(
                &page[0].wire,
                Some("history-page-1".to_owned()),
                false,
            )
            .is_err()
    );
    assert_eq!(runtime.local_source_pending_segments().len(), 1);
    assert!(
        runtime
            .local_history_backfill_inflight_checkpoint()
            .is_some()
    );
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 1);

    storage
        .set_query_only_for_test(false)
        .expect("disable SQLite write failure");
    runtime
        .acknowledge_local_source_segment_and_checkpoint_backfill(
            &page[0].wire,
            Some("history-page-1".to_owned()),
            false,
        )
        .expect("retry backfill acknowledgement");
    assert!(runtime.local_source_pending_segments().is_empty());
    assert!(
        runtime
            .local_history_backfill_inflight_checkpoint()
            .is_some()
    );
    assert!(storage.source_delivery_journal().unwrap().is_empty());
}

#[test]
fn source_journal_replays_rows_beyond_the_bounded_restart_window() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    for sequence in 0..300_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }
    drop(runtime);

    let mut restored = load(temporary.path());
    for _ in 0..300 {
        let front = restored
            .local_source_pending_segments()
            .into_iter()
            .next()
            .expect("durable backlog row becomes replayable");
        restored
            .acknowledge_local_source_segment(&front.wire)
            .expect("acknowledge durable backlog row");
    }
    assert!(restored.local_source_pending_segments().is_empty());
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    assert!(storage.source_delivery_journal().unwrap().is_empty());
}

#[test]
fn source_journal_restarts_same_timestamp_segments_in_cursor_order() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let rows = (0..32_u64)
        .map(|sequence| {
            let segment = CanonicalSegment::new(
                "cluster-a",
                Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                None,
                100,
                100,
            )
            .expect("segment")
            .sign(&key)
            .expect("signature");
            let wire = segment.wire_bytes().expect("wire");
            crate::state::history_storage::SourceDeliveryJournalRow {
                id: hex::encode(Sha256::digest(&wire)),
                stream: "runtime".to_owned(),
                closed_at_unix_seconds: 100,
                identity: source_identity.clone(),
                wire,
            }
        })
        .collect::<Vec<_>>();
    storage
        .append_source_delivery_journal(&rows)
        .expect("append same timestamp rows");
    storage
        .write(crate::state::history_storage::REPOSITORY_REPLICA_KEY, b"{}")
        .expect("simulate lost control snapshot");

    let mut restored = load(temporary.path());
    for sequence in 0..32_u64 {
        let front = restored
            .local_source_pending_segments()
            .into_iter()
            .next()
            .expect("pending segment");
        let actual = SignedSegment::from_wire(&front.wire)
            .expect("signed segment")
            .canonical()
            .first_cursor()
            .sequence();
        assert_eq!(actual, sequence);
        restored
            .acknowledge_local_source_segment(&front.wire)
            .expect("acknowledge segment");
    }
}

#[test]
fn migration_fallback_preserves_legacy_source_delivery_pending_rows() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = load(temporary.path());
    let queued = runtime
        .queue_local_source_segment(
            "cluster-a",
            source_identity,
            &key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"pending-before-fallback".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            100,
        )
        .expect("queue source segment")
        .expect("queued source segment");
    runtime.snapshot.records.push(StoredRecord {
        observed_at_unix_seconds: 100,
        received_at_unix_seconds: 100,
        source_node_id: "node-a".to_owned(),
        source_epoch: i64::MAX as u64 + 1,
        stream: "runtime".to_owned(),
        sequence: 0,
        subject_node_id: "node-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "runtime.v1".to_owned(),
        schema_version: 1,
        record_key: b"invalid-migration-epoch".to_vec(),
        payload: b"sample".to_vec(),
        tombstone: false,
    });
    let snapshot = serde_json::to_vec(&runtime.snapshot).expect("serialize legacy snapshot");
    storage
        .write(
            crate::state::history_storage::REPOSITORY_REPLICA_KEY,
            &snapshot,
        )
        .expect("write legacy snapshot");
    drop(runtime);

    let reloaded = RepositoryReplicaRuntime::load(storage.clone()).expect("fallback load");
    drop(reloaded);
    drop(storage);

    let fallback_storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let restored = RepositoryReplicaRuntime::load(fallback_storage).expect("reload JSON fallback");
    let front = restored
        .local_source_pending_segments()
        .into_iter()
        .next()
        .expect("pending row survives fallback");
    assert_eq!(front.wire, queued.wire);
}
