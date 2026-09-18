use super::{
    HistoricalBackfillCollector, HistoricalBackfillPageCursor, HistoricalBackfillSortKey,
    InitialBackfillProgress, RepositoryInitialBackfillPage, RepositoryInitialBackfillRecord,
    is_initial_history_source_peer, next_historical_backfill_page_cursor,
    serialized_backfill_record_bytes, validate_peer_backfill_page,
};
use crate::history_sync::SyncRecord;
use crate::state::history_repository::{
    MAX_INITIAL_BACKFILL_PAGE_BYTES, MAX_INITIAL_BACKFILL_PAGE_RECORDS,
};

fn backfill_record(payload_base64: String) -> RepositoryInitialBackfillRecord {
    RepositoryInitialBackfillRecord {
        observed_at_unix_seconds: 1,
        source_node_id: None,
        source_epoch: None,
        stream: None,
        sequence: None,
        subject_node_id: "node-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "runtime.v1".to_owned(),
        schema_version: 1,
        record_key_base64: "a2V5".to_owned(),
        payload_base64,
        tombstone: false,
    }
}

#[test]
fn peer_backfill_rejects_an_oversized_record_page() {
    let page = RepositoryInitialBackfillPage {
        records: (0..=MAX_INITIAL_BACKFILL_PAGE_RECORDS)
            .map(|_| backfill_record(String::new()))
            .collect(),
        next_page_cursor: None,
    };

    assert!(validate_peer_backfill_page(&page, None).is_err());
}

#[test]
fn peer_backfill_rejects_a_page_over_the_byte_budget() {
    let page = RepositoryInitialBackfillPage {
        records: vec![backfill_record("x".repeat(MAX_INITIAL_BACKFILL_PAGE_BYTES))],
        next_page_cursor: None,
    };

    assert!(validate_peer_backfill_page(&page, None).is_err());
}

#[test]
fn peer_backfill_rejects_a_malformed_or_regressing_cursor() {
    let page = |next_page_cursor| RepositoryInitialBackfillPage {
        records: Vec::new(),
        next_page_cursor,
    };
    assert!(validate_peer_backfill_page(&page(Some("invalid".to_owned())), None).is_err());

    let previous = HistoricalBackfillPageCursor {
        after: HistoricalBackfillSortKey {
            observed_at_unix_seconds: 10,
            schema_id: "runtime.v1".to_owned(),
            record_key: b"key-10".to_vec(),
        },
        snapshot_end_unix_seconds: Some(100),
    };
    let regressing = HistoricalBackfillPageCursor {
        after: HistoricalBackfillSortKey {
            observed_at_unix_seconds: 9,
            schema_id: "runtime.v1".to_owned(),
            record_key: b"key-9".to_vec(),
        },
        snapshot_end_unix_seconds: Some(100),
    };
    let cursor = regressing.encode().expect("cursor encoding");
    assert!(validate_peer_backfill_page(&page(Some(cursor)), Some(&previous)).is_err());
}

#[test]
fn tick_progress_keeps_peer_pages_eligible_while_local_backfill_runs() {
    let progress = InitialBackfillProgress::Complete
        .combine(InitialBackfillProgress::InProgress)
        .combine(InitialBackfillProgress::InProgress)
        .combine(InitialBackfillProgress::Complete);

    assert_eq!(progress, InitialBackfillProgress::InProgress);
}

#[test]
fn tick_progress_blocks_readiness_after_an_unavailable_peer_page() {
    let progress = InitialBackfillProgress::Complete
        .combine(InitialBackfillProgress::InProgress)
        .combine(InitialBackfillProgress::Unavailable)
        .combine(InitialBackfillProgress::Complete);

    assert_eq!(progress, InitialBackfillProgress::Unavailable);
}

#[test]
fn initial_backfill_includes_configured_repository_sources() {
    assert!(is_initial_history_source_peer(
        "ordinary-source",
        "repository-a"
    ));
    assert!(is_initial_history_source_peer(
        "repository-a",
        "repository-b"
    ));
    assert!(is_initial_history_source_peer(
        "repository-b",
        "repository-a"
    ));
    assert!(!is_initial_history_source_peer(
        "repository-a",
        "repository-a"
    ));
}

#[test]
fn initial_backfill_collector_bounds_pages_below_the_record_limit() {
    let mut collected = HistoricalBackfillCollector::new(None, 128);
    for observed_at in [1_u64, 2, 3] {
        collected
            .push((
                observed_at,
                SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    observed_at.to_be_bytes().to_vec(),
                    vec![u8::try_from(observed_at).expect("small test value"); 60 * 1024],
                    false,
                ),
            ))
            .expect("bounded historical record");
    }

    let page_bytes = collected
        .records
        .values()
        .map(|(_, record)| serialized_backfill_record_bytes(record))
        .sum::<anyhow::Result<usize>>()
        .expect("page size");

    assert!(collected.has_more);
    assert_eq!(collected.records.len(), 2);
    assert!(page_bytes <= MAX_INITIAL_BACKFILL_PAGE_BYTES);
}

#[test]
fn initial_backfill_cursor_freezes_the_history_snapshot_tail() {
    let mut first_page = HistoricalBackfillCollector::new(None, 2).with_snapshot_end(100);
    for observed_at in [50, 60, 70, 101] {
        first_page
            .push((
                observed_at,
                SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    observed_at.to_be_bytes().to_vec(),
                    Vec::new(),
                    false,
                ),
            ))
            .expect("historical record");
    }

    let cursor = first_page
        .next_cursor()
        .expect("cursor encoding")
        .expect("more frozen history");
    let cursor = HistoricalBackfillPageCursor::decode(&cursor).expect("cursor decoding");
    assert_eq!(cursor.snapshot_end_unix_seconds, Some(100));

    let mut next_page =
        HistoricalBackfillCollector::new(Some(cursor.after), 2).with_snapshot_end(100);
    for observed_at in [50, 60, 70, 101] {
        next_page
            .push((
                observed_at,
                SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    observed_at.to_be_bytes().to_vec(),
                    Vec::new(),
                    false,
                ),
            ))
            .expect("historical record");
    }

    assert_eq!(next_page.records.len(), 1);
    assert!(!next_page.has_more);
    assert_eq!(
        next_page
            .records
            .first_key_value()
            .expect("remaining record")
            .1
            .0,
        70
    );
}

#[test]
fn initial_backfill_page_cursor_accepts_the_legacy_sort_key() {
    let legacy = HistoricalBackfillSortKey {
        observed_at_unix_seconds: 100,
        schema_id: "runtime.v1".to_owned(),
        record_key: b"node-history:node:node-a:100".to_vec(),
    };

    let decoded = HistoricalBackfillPageCursor::decode(&legacy.encode().expect("legacy cursor"))
        .expect("legacy cursor decoding");

    assert_eq!(decoded.after.observed_at_unix_seconds, 100);
    assert_eq!(decoded.after.schema_id, "runtime.v1");
    assert_eq!(
        decoded.after.record_key,
        b"node-history:node:node-a:100".to_vec()
    );
    assert_eq!(decoded.snapshot_end_unix_seconds, None);
}

#[test]
fn initial_backfill_page_response_keeps_the_frozen_snapshot_tail() {
    let mut collected = HistoricalBackfillCollector::new(None, 3).with_snapshot_end(100);
    for observed_at in [50, 60, 70] {
        collected
            .push((
                observed_at,
                SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    observed_at.to_be_bytes().to_vec(),
                    Vec::new(),
                    false,
                ),
            ))
            .expect("historical record");
    }
    let after = collected.records.first_key_value().expect("first record").0;

    let cursor = next_historical_backfill_page_cursor(&collected, 1, Some(after))
        .expect("page cursor encoding")
        .expect("more records in the frozen snapshot");
    let cursor = HistoricalBackfillPageCursor::decode(&cursor).expect("page cursor decoding");

    assert_eq!(cursor.snapshot_end_unix_seconds, Some(100));
}
