use super::{tests::load, *};

#[test]
fn sqlite_marks_aggregate_without_bucket_range_incomplete() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    let observed_at = 1_000_u64;
    let row = StoredRecord {
        observed_at_unix_seconds: observed_at,
        received_at_unix_seconds: observed_at,
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "traffic".to_owned(),
        sequence: 1,
        subject_node_id: "subject-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "traffic.v1".to_owned(),
        schema_version: 1,
        record_key: b"aggregate".to_vec(),
        payload: serde_json::to_vec(&serde_json::json!({
            "algorithm": "sha256",
            "resolution": "hour",
            "record_count": 1,
            "first_sequence": 1,
            "last_sequence": 1,
            "payload_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "complete": true,
        }))
        .expect("aggregate payload"),
        tombstone: false,
    }
    .sqlite_row()
    .expect("SQLite row");
    assert_eq!(row.aggregate_complete, Some(false));
    assert_eq!(row.aggregate_start_unix_seconds, None);
    assert_eq!(row.aggregate_end_unix_seconds, None);
    runtime
        .storage
        .upsert_repository_history_records(&[row])
        .expect("seed aggregate without range");
    let gap = runtime
        .incomplete_aggregate_gap(&HistoryQuery::new(999, 1_001, 10).expect("history query"))
        .expect("missing range is observable as a gap");
    assert_eq!(gap, Some((observed_at, observed_at)));

    let raw_row = StoredRecord {
        observed_at_unix_seconds: 2_000,
        received_at_unix_seconds: 2_000,
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "traffic".to_owned(),
        sequence: 2,
        subject_node_id: "subject-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "traffic.v1".to_owned(),
        schema_version: 1,
        record_key: b"raw".to_vec(),
        payload: b"raw payload".to_vec(),
        tombstone: false,
    }
    .sqlite_row()
    .expect("raw SQLite row");
    assert_eq!(raw_row.aggregate_complete, Some(true));
    assert_eq!(raw_row.aggregate_start_unix_seconds, None);
    assert_eq!(raw_row.aggregate_end_unix_seconds, None);
    runtime
        .storage
        .upsert_repository_history_records(&[raw_row])
        .expect("seed raw row");
    let raw_gap = runtime
        .incomplete_aggregate_gap(&HistoryQuery::new(1_999, 2_001, 10).expect("raw history query"))
        .expect("raw row does not create a gap");
    assert_eq!(raw_gap, None);

    let now = observed_at
        .saturating_add(
            super::super::RepositoryRetentionPolicy::default().minute_retention_seconds(),
        )
        .saturating_add(2);
    runtime
        .prepare_for_replication(now)
        .expect("compact incomplete aggregate");
    let compacted_gap = runtime
        .incomplete_aggregate_gap(&HistoryQuery::new(0, now, 10).expect("compacted history query"))
        .expect("incomplete aggregate stays incomplete after compaction");
    assert!(compacted_gap.is_some());
}

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
