use super::{tests::load, *};

#[test]
fn sqlite_retention_expiry_invalidates_summary_after_unchanged_page() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let now = policy
        .max_age_seconds()
        .saturating_add(policy.minute_retention_seconds())
        .saturating_add(10_000);
    let make_row = |observed_at, sequence| {
        StoredRecord {
            observed_at_unix_seconds: observed_at,
            received_at_unix_seconds: now,
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
            sequence,
            subject_node_id: "subject-a".to_owned(),
            observer_node_id: "node-a".to_owned(),
            schema_id: "runtime.v1".to_owned(),
            schema_version: 1,
            record_key: sequence.to_be_bytes().to_vec(),
            payload: b"sample".to_vec(),
            tombstone: false,
        }
        .sqlite_row()
        .expect("SQLite row")
    };
    let mut runtime = load(temporary.path());
    let retained_row = make_row(now.saturating_sub(policy.minute_retention_seconds() + 2), 2);
    runtime
        .storage
        .upsert_repository_history_records(&[retained_row])
        .expect("seed retained row");
    runtime
        .prepare_for_replication(now)
        .expect("compact retained row");

    let expired_row = make_row(0, 1);
    runtime
        .storage
        .upsert_repository_history_records(&[expired_row.clone()])
        .expect("seed expired row before cursor");
    runtime.snapshot.retention_compaction_cursor =
        Some(RetentionCompactionCursor::from(&expired_row));
    runtime.snapshot.partition_summaries_complete = true;
    runtime
        .snapshot
        .deep_verified_peer_ids
        .insert("repository-peer".to_owned());

    runtime
        .prepare_for_replication(now)
        .expect("expire row during unchanged retention page");
    assert!(!runtime.snapshot.partition_summaries_complete);
    assert!(runtime.snapshot.deep_verified_peer_ids.is_empty());
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("retained count"),
        1
    );
}
