use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use super::{RepositoryHistoryCompactionCursor, StoredRecord, tests::load};

#[test]
fn tiered_sqlite_history_restarts_a_legacy_cursor_with_tombstones_first() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let mut rows = (0..2_u64)
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: 1_000 + sequence,
                received_at_unix_seconds: 2_000,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("legacy-{sequence}").into_bytes(),
                payload: b"legacy".to_vec(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite row")
        })
        .collect::<Vec<_>>();
    rows.push(
        StoredRecord {
            observed_at_unix_seconds: 1_002,
            received_at_unix_seconds: 2_000,
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "tombstone".to_owned(),
            sequence: 2,
            subject_node_id: "subject-a".to_owned(),
            observer_node_id: "node-a".to_owned(),
            schema_id: "traffic.v1".to_owned(),
            schema_version: 1,
            record_key: b"legacy-deleted".to_vec(),
            payload: Vec::new(),
            tombstone: true,
        }
        .sqlite_row()
        .expect("SQLite tombstone row"),
    );
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed legacy cursor rows");
    let legacy_cursor = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&RepositoryHistoryCompactionCursor::from(&rows[0]))
            .expect("legacy cursor"),
    );

    let page = runtime
        .tiered_backfill_page(Some(&legacy_cursor), 8, u64::MAX, u64::MAX)
        .expect("restart legacy cursor");
    assert_eq!(page.records.len(), 1);
    assert!(page.records[0].tombstone);
    assert_eq!(page.records[0].record_key, b"legacy-deleted");
}
