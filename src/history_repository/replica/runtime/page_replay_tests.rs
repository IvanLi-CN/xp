use super::{
    tests::{identity, load, record, signing_key},
    *,
};

#[test]
fn local_history_backfill_page_replays_all_segments_before_checkpoint() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let page = runtime
        .queue_local_history_backfill_batches(
            "cluster-a",
            identity.clone(),
            &key,
            Some("history-page-2".to_owned()),
            false,
            vec![
                (vec![record(b"history-1", false)], 1_000),
                (vec![record(b"history-2", false)], 1_001),
            ],
        )
        .expect("queue multi-segment page");
    assert_eq!(page.len(), 2);
    runtime
        .receive_wire("cluster-a", &page[0].identity, &page[0].wire, 1_010)
        .expect("receive first segment before interruption");

    let mut restarted = load(temporary.path());
    let replay = restarted
        .local_history_backfill_inflight_segments()
        .expect("restore inflight page");
    assert_eq!(replay.len(), 2);
    for segment in &replay {
        restarted
            .receive_wire("cluster-a", &segment.identity, &segment.wire, 1_011)
            .expect("replay page segment");
    }
    restarted
        .acknowledge_local_source_segments_and_checkpoint_backfill(
            &replay,
            Some("history-page-2".to_owned()),
            false,
        )
        .expect("acknowledge complete page");

    let restored = load(temporary.path());
    assert!(restored.local_source_pending_segments().is_empty());
    assert_eq!(
        restored.local_history_backfill_cursor(),
        Some("history-page-2")
    );
    assert!(
        restored
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    assert_eq!(
        restored
            .storage
            .repository_history_record_count()
            .expect("history count"),
        2,
        "replaying a partially delivered page must not duplicate rows"
    );
}
