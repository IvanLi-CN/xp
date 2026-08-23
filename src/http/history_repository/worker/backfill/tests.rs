use super::{
    HistoricalBackfillCollector, HistoricalBackfillPageCursor, HistoricalBackfillSortKey,
    InitialBackfillProgress, is_initial_history_source_peer,
};
use crate::history_sync::SyncRecord;
use std::collections::BTreeSet;

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
fn initial_backfill_excludes_every_configured_repository_source() {
    let repository_node_ids =
        BTreeSet::from(["repository-a".to_owned(), "repository-b".to_owned()]);

    assert!(is_initial_history_source_peer(
        "ordinary-source",
        &repository_node_ids
    ));
    assert!(!is_initial_history_source_peer(
        "repository-a",
        &repository_node_ids
    ));
    assert!(!is_initial_history_source_peer(
        "repository-b",
        &repository_node_ids
    ));
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
