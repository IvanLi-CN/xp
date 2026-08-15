use super::{tests::load, *};

#[test]
fn tiered_backfill_emits_tombstones_before_older_history_across_pages() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let rows = [
        StoredRecord {
            observed_at_unix_seconds: 100,
            received_at_unix_seconds: 1,
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "traffic".to_owned(),
            sequence: 0,
            subject_node_id: "subject-a".to_owned(),
            observer_node_id: "node-a".to_owned(),
            schema_id: "traffic.v1".to_owned(),
            schema_version: 1,
            record_key: b"node-history:node:subject-a:old".to_vec(),
            payload: b"old".to_vec(),
            tombstone: false,
        },
        StoredRecord {
            observed_at_unix_seconds: 10_000,
            received_at_unix_seconds: 1,
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "tombstone".to_owned(),
            sequence: 0,
            subject_node_id: "subject-a".to_owned(),
            observer_node_id: "node-a".to_owned(),
            schema_id: "traffic.v1".to_owned(),
            schema_version: 1,
            record_key: b"node-history:node:subject-a:".to_vec(),
            payload: b"deleted".to_vec(),
            tombstone: true,
        },
    ]
    .into_iter()
    .map(|record| record.sqlite_row().expect("SQLite row"))
    .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed tombstone and history");

    let first = runtime
        .tiered_backfill_page(None, 1, 5_000, 10_000)
        .expect("tombstone page");
    assert_eq!(first.records.len(), 1);
    assert!(first.records[0].tombstone);
    let second = runtime
        .tiered_backfill_page(first.next_cursor.as_deref(), 1, 5_000, 10_000)
        .expect("history page");
    assert_eq!(second.records.len(), 1);
    assert!(!second.records[0].tombstone);
    assert!(second.next_cursor.is_none());
}
