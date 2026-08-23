use super::{InitialBackfillProgress, is_initial_history_source_peer};
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
