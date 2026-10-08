use super::*;

#[test]
fn sequence_summary_digest_is_independent_of_replica_arrival_time() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let summarize = |path: &std::path::Path, received_at| {
        let storage = HistoryStorage::open(path);
        let row = RepositoryHistoryRecordRow {
            source_node_id: "source".to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            sequence: 1,
            subject_node_id: "subject".to_owned(),
            observer_node_id: "observer".to_owned(),
            schema_id: "schema".to_owned(),
            schema_version: 1,
            record_key: b"key".to_vec(),
            tombstone: false,
            observed_start_unix_seconds: 10,
            observed_end_unix_seconds: 10,
            received_at_unix_seconds: received_at,
            aggregate_complete: Some(true),
            aggregate_start_unix_seconds: Some(10),
            aggregate_end_unix_seconds: Some(10),
            payload: b"same canonical history".to_vec(),
        };
        storage.upsert_repository_history_records(&[row]).unwrap();
        storage
            .rebuild_repository_history_sequence_summary_block("source", 1, "runtime", 0)
            .unwrap();
        storage
            .repository_history_sequence_summary_blocks()
            .unwrap()
    };
    assert_eq!(summarize(first.path(), 11), summarize(second.path(), 999));
}

#[test]
fn sequence_summary_block_rebuild_is_incremental_and_tombstone_safe() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    storage
        .write(REPOSITORY_REPLICA_KEY, br#"{"external_history":true}"#)
        .unwrap();
    let row = |sequence: u64, payload: &[u8]| RepositoryHistoryRecordRow {
        source_node_id: "source".to_owned(),
        source_epoch: 1,
        stream: "runtime".to_owned(),
        sequence,
        subject_node_id: "subject".to_owned(),
        observer_node_id: "observer".to_owned(),
        schema_id: "schema".to_owned(),
        schema_version: 1,
        record_key: b"key".to_vec(),
        tombstone: false,
        observed_start_unix_seconds: sequence,
        observed_end_unix_seconds: sequence,
        received_at_unix_seconds: sequence,
        aggregate_complete: Some(true),
        aggregate_start_unix_seconds: Some(sequence),
        aggregate_end_unix_seconds: Some(sequence),
        payload: payload.to_vec(),
    };
    storage
        .upsert_repository_history_records(&[row(0, b"zero"), row(1, b"one"), row(4096, b"two")])
        .unwrap();
    let (migrated, _, complete) = storage
        .repository_history_sequence_summary_migration_page(None, 16)
        .unwrap();
    assert!(complete);
    assert!(migrated.contains(&("source".to_owned(), 1, "runtime".to_owned(), 0)));
    let dirty = storage
        .repository_history_dirty_sequence_summary_blocks(16)
        .unwrap();
    assert_eq!(dirty.len(), 2);
    for (_, epoch, stream, block) in dirty {
        storage
            .rebuild_repository_history_sequence_summary_block("source", epoch, &stream, block)
            .unwrap();
    }
    let blocks = storage
        .repository_history_sequence_summary_blocks()
        .unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].record_count, 2);

    // Reopen an actual predecessor schema with clean, unversioned digests. Historical rows
    // stay in the same database; only metadata becomes unavailable until bounded rebuilding.
    drop(storage);
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3")).unwrap();
    connection
        .execute(
            "ALTER TABLE repository_history_sequence_summary_blocks
        DROP COLUMN digest_version",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE repository_history_sequence_summary_blocks
        SET digest = zeroblob(32), dirty = 0",
            [],
        )
        .unwrap();
    drop(connection);
    let storage = HistoryStorage::open(temporary.path());
    assert_eq!(storage.repository_history_record_count().unwrap(), 3);
    assert!(
        storage
            .repository_history_sequence_summary_blocks()
            .unwrap()
            .is_empty()
    );
    let dirty = storage
        .repository_history_dirty_sequence_summary_blocks(16)
        .unwrap();
    assert_eq!(dirty.len(), 2);
    for (source, epoch, stream, block) in dirty {
        storage
            .rebuild_repository_history_sequence_summary_block(&source, epoch, &stream, block)
            .unwrap();
    }
    assert_eq!(
        storage
            .repository_history_sequence_summary_blocks()
            .unwrap(),
        blocks
    );
    // An older binary can overwrite digest without touching the additive version column.
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3")).unwrap();
    connection
        .execute(
            "UPDATE repository_history_sequence_summary_blocks
        SET digest = zeroblob(32), dirty = 0",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(
        storage
            .repository_history_sequence_summary_blocks()
            .unwrap()
            .is_empty()
    );
    for (source, epoch, stream, block) in storage
        .repository_history_dirty_sequence_summary_blocks(16)
        .unwrap()
    {
        storage
            .rebuild_repository_history_sequence_summary_block(&source, epoch, &stream, block)
            .unwrap();
    }
    assert_eq!(
        storage
            .repository_history_sequence_summary_blocks()
            .unwrap(),
        blocks
    );

    let mut tombstone = row(4096, b"");
    tombstone.tombstone = true;
    storage
        .upsert_repository_history_records(&[tombstone])
        .unwrap();
    storage
        .rebuild_repository_history_sequence_summary_block("source", 1, "runtime", 1)
        .unwrap();
    storage
        .delete_repository_history_tombstone(&RepositoryHistoryTombstone {
            source_node_id: "source".to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            subject_node_id: "subject".to_owned(),
            observer_node_id: "observer".to_owned(),
            schema_id: "schema".to_owned(),
            schema_version: 1,
            record_key: b"key".to_vec(),
            prefix: false,
        })
        .unwrap();
    assert!(
        storage
            .repository_history_dirty_sequence_summary_blocks(16)
            .unwrap()
            .is_empty()
    );

    let mut changed = row(0, b"zero");
    changed.aggregate_start_unix_seconds = Some(99);
    storage
        .upsert_repository_history_records(&[changed])
        .unwrap();
    assert_eq!(
        storage
            .repository_history_dirty_sequence_summary_blocks(16)
            .unwrap(),
        vec![("source".to_owned(), 1, "runtime".to_owned(), 0)]
    );

    storage
        .delete_repository_history_for_tombstone(&RepositoryHistoryTombstone {
            source_node_id: "source".to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            subject_node_id: "subject".to_owned(),
            observer_node_id: "observer".to_owned(),
            schema_id: "schema".to_owned(),
            schema_version: 1,
            record_key: b"key".to_vec(),
            prefix: false,
        })
        .unwrap();
    let dirty = storage
        .repository_history_dirty_sequence_summary_blocks(16)
        .unwrap();
    assert_eq!(dirty.len(), 1);
    for (_, epoch, stream, block) in dirty {
        storage
            .rebuild_repository_history_sequence_summary_block("source", epoch, &stream, block)
            .unwrap();
    }
    assert!(
        storage
            .repository_history_sequence_summary_blocks()
            .unwrap()
            .is_empty()
    );
}
