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

#[test]
fn tiered_backfill_aggregate_keyset_preserves_every_row_across_pages() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let rows =
        (0..5)
            .map(|sequence| {
                StoredRecord {
        observed_at_unix_seconds: 120 + (4 - sequence) * 10,
        received_at_unix_seconds: 1,
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "traffic".to_owned(),
        sequence,
        subject_node_id: "subject-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "traffic.v1".to_owned(),
        schema_version: 1,
        record_key: format!("key-{sequence}").into_bytes(),
        payload: serde_json::to_vec(&serde_json::json!({
            "algorithm": "sha256", "resolution": "5m",
            "bucket_start_unix_seconds": 100, "bucket_end_unix_seconds": 199,
            "record_count": 2, "first_sequence": sequence, "last_sequence": sequence + 1,
            "payload_sha256": "abc", "complete": true
        })).expect("aggregate payload"),
        tombstone: false,
    }.sqlite_row().expect("SQLite row")
            })
            .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed aggregate rows");
    let mut cursor = None;
    let mut sequences = Vec::new();
    loop {
        let page = runtime
            .tiered_backfill_page(cursor.as_deref(), 2, 500, 1_000)
            .expect("aggregate page");
        for record in &page.records {
            assert_eq!(
                record.observed_at_unix_seconds,
                120 + (4 - record.sequence) * 10
            );
            sequences.push(record.sequence);
        }
        if sequences.len() == 2 {
            use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
            let mut old: serde_json::Value = serde_json::from_slice(
                &URL_SAFE_NO_PAD
                    .decode(page.next_cursor.as_ref().expect("continuation"))
                    .expect("cursor bytes"),
            )
            .expect("cursor JSON");
            assert_eq!(old["keyset_version"], 2);
            assert_eq!(old["after"]["observed_start_unix_seconds"], 100);
            old.as_object_mut()
                .expect("object")
                .remove("keyset_version");
            old["after"]["observed_start_unix_seconds"] = serde_json::json!(150);
            let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&old).expect("old cursor"));
            assert!(matches!(
                runtime.tiered_backfill_page(Some(&encoded), 2, 500, 1_000),
                Err(RepositoryRuntimeError::StateLimitExceeded)
            ));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
        assert!(sequences.len() <= 5, "export must make bounded progress");
    }
    assert_eq!(sequences, vec![0, 1, 2, 3, 4]);
}

#[test]
fn tiered_backfill_rejects_old_raw_time_continuation_before_seeking() {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let old = serde_json::json!({
        "repair_cache_cutoff_unix_seconds": 500,
        "received_at_cutoff_unix_seconds": 1,
        "tombstone_high_watermark": null, "record_high_watermark": null,
        "phase": "records", "export_session_id": "old-export",
        "after": {"observed_start_unix_seconds": 150, "source_node_id": "node-a",
            "source_epoch": 7, "stream": "traffic", "sequence": 1}
    });
    let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&old).expect("old cursor"));
    assert!(matches!(
        runtime.tiered_backfill_page(Some(&encoded), 2, 500, 1_000),
        Err(RepositoryRuntimeError::StateLimitExceeded)
    ));
    let mut unknown = old;
    unknown["keyset_version"] = serde_json::json!(3);
    let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&unknown).expect("unknown cursor"));
    assert!(validate_tiered_backfill_cursor(&encoded, None).is_err());
}
