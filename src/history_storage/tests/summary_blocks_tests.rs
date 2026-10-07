use super::*;

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
