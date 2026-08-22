use super::InitialBackfillProgress;

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
