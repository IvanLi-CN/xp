use super::super::{StoredRecord, tests::load};

#[test]
fn sequence_summary_migration_survives_late_writes_and_restart() {
    let temporary = tempfile::tempdir().unwrap();
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = true;
    runtime.snapshot.legacy_segment_cursor_index_complete = true;
    let rows = (0..5000)
        .map(|sequence| StoredRecord {
            observed_at_unix_seconds: sequence,
            received_at_unix_seconds: sequence + 1,
            source_node_id: "source".to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            sequence,
            subject_node_id: "subject".to_owned(),
            observer_node_id: "observer".to_owned(),
            schema_id: "runtime.v1".to_owned(),
            schema_version: 1,
            record_key: sequence.to_be_bytes().to_vec(),
            payload: b"retained".to_vec(),
            tombstone: false,
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(
            &rows
                .iter()
                .map(|row| row.sqlite_row().unwrap())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    runtime
        .advance_sequence_summary_block_rebuild_page()
        .unwrap();
    let cursor = runtime.snapshot.sequence_summary_migration_cursor.clone();
    assert_eq!(cursor.as_ref().unwrap().sequence, 4095);
    runtime.snapshot.partition_summary_cursor = cursor.clone();
    let mut late = rows[0].clone();
    late.payload = b"late correction".to_vec();
    runtime
        .storage
        .upsert_repository_history_records(&[late.sqlite_row().unwrap()])
        .unwrap();
    runtime.update_partition_summary_for_record(&late).unwrap();
    assert_eq!(
        serde_json::to_value(&runtime.snapshot.sequence_summary_migration_cursor).unwrap(),
        serde_json::to_value(&cursor).unwrap()
    );
    runtime.persist_control_state().unwrap();
    drop(runtime);
    let mut runtime = load(temporary.path());
    assert_eq!(
        serde_json::to_value(&runtime.snapshot.sequence_summary_migration_cursor).unwrap(),
        serde_json::to_value(&cursor).unwrap()
    );
    for _ in 0..4 {
        runtime
            .advance_sequence_summary_block_rebuild_page()
            .unwrap();
    }
    assert!(runtime.sequence_summary_blocks_ready());
    assert!(!runtime.partition_summaries_ready());
    let summary = runtime.replication_summary_after(None, true).unwrap();
    assert_eq!(summary.summary_version, 3);
    assert!(summary.partitions_included);
    let mut unknown = summary.clone();
    unknown.summary_version = 4;
    assert!(!runtime.retained_summary_available(&unknown));
    for requested in [None, Some(1), Some(2), Some(4)] {
        let legacy = runtime
            .replication_summary_for_version(None, true, requested)
            .unwrap();
        assert_eq!(legacy.summary_version, 1);
        assert!(legacy.sequence_blocks.is_empty());
        assert!(!legacy.partitions_included);
    }
    assert_eq!(
        summary
            .sequence_blocks
            .iter()
            .map(|block| block.record_count)
            .sum::<u64>(),
        5000
    );
    runtime.mark_history_truncated().unwrap();
    let summary = runtime.replication_summary_after(None, true).unwrap();
    assert!(runtime.requires_repair(&summary, true).unwrap());
    assert!(!runtime.requires_retained_repair(&summary).unwrap());
    assert!(runtime.retained_partitions_converged(&summary).unwrap());
    assert!(runtime.history_is_truncated());
}
