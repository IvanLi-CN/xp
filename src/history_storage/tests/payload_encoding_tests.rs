use super::*;

fn record(sequence: u64) -> RepositoryHistoryRecordRow {
    RepositoryHistoryRecordRow {
        source_node_id: "source".into(),
        source_epoch: 1,
        stream: "runtime".into(),
        sequence,
        subject_node_id: "subject".into(),
        observer_node_id: "source".into(),
        schema_id: "runtime.v1".into(),
        schema_version: 1,
        record_key: sequence.to_be_bytes().to_vec(),
        tombstone: false,
        observed_start_unix_seconds: sequence,
        observed_end_unix_seconds: sequence,
        received_at_unix_seconds: sequence,
        aggregate_complete: Some(true),
        aggregate_start_unix_seconds: None,
        aggregate_end_unix_seconds: None,
        payload: vec![b'x'; 4096],
    }
}

#[test]
fn payload_migration_resumes_past_a_full_page_and_keeps_bytes_counts_and_digests() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let records = (0..270).map(record).collect::<Vec<_>>();
    storage.upsert_repository_history_records(&records).unwrap();
    {
        let mut backend = storage.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            panic!("sqlite");
        };
        // Model an existing database written before the codec was introduced.
        connection
            .execute(
                "UPDATE repository_history_records SET payload=?1",
                [&records[0].payload],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO repository_history_segments(id,closed_at,payload)
             VALUES('signed-wire',1,?1)",
                [&records[0].payload],
            )
            .unwrap();
    }
    storage
        .rebuild_repository_history_sequence_summary_block("source", 1, "runtime", 0)
        .unwrap();
    let digest = storage
        .repository_history_sequence_summary_blocks()
        .unwrap();
    assert_eq!(storage.compact_repository_payload_page().unwrap(), 129);
    drop(storage);
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(storage.compact_repository_payload_page().unwrap(), 128);
    assert_eq!(storage.compact_repository_payload_page().unwrap(), 14);
    assert_eq!(storage.compact_repository_payload_page().unwrap(), 0);
    assert_eq!(storage.repository_history_record_count().unwrap(), 270);
    assert_eq!(storage.repository_history_segment_count().unwrap(), 1);
    let decoded = storage
        .repository_history_records(None, Some(0), Some(1000), 0, 300)
        .unwrap();
    assert_eq!(decoded.len(), 270);
    assert!(decoded.iter().all(|row| row.payload == records[0].payload));
    let segments = storage.repository_history_segments_page(None, 10).unwrap();
    assert_eq!(segments[0].payload, records[0].payload);
    storage
        .rebuild_repository_history_sequence_summary_block("source", 1, "runtime", 0)
        .unwrap();
    assert_eq!(
        storage
            .repository_history_sequence_summary_blocks()
            .unwrap(),
        digest
    );
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("sqlite");
    };
    let size: usize = connection
        .query_row(
            "SELECT sum(length(payload)) FROM repository_history_records",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(size < 270 * 4096 / 10);
}

#[test]
fn payload_migration_failure_rolls_back_rows_and_cursor_together() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let records = (0..3).map(record).collect::<Vec<_>>();
    storage.upsert_repository_history_records(&records).unwrap();
    {
        let mut backend = storage.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            panic!("sqlite");
        };
        connection
            .execute(
                "UPDATE repository_history_records SET payload=?1",
                [&records[0].payload],
            )
            .unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER block_payload_rewrite BEFORE UPDATE OF payload
             ON repository_history_records WHEN OLD.sequence=1
             BEGIN SELECT RAISE(ABORT,'injected migration failure'); END;",
            )
            .unwrap();
    }
    assert!(storage.compact_repository_payload_page().is_err());
    {
        let mut backend = storage.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            panic!("sqlite");
        };
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM history_storage_meta
             WHERE key='repository_payload_records_cursor_v1'",
                    [],
                    |row| row.get::<_, usize>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT sum(length(payload)) FROM repository_history_records",
                    [],
                    |row| row.get::<_, usize>(0)
                )
                .unwrap(),
            3 * 4096
        );
        connection
            .execute_batch("DROP TRIGGER block_payload_rewrite")
            .unwrap();
    }
    assert_eq!(storage.compact_repository_payload_page().unwrap(), 3);
}
