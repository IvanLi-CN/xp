use super::*;

#[test]
fn stale_local_history_backfill_preparation_clears_the_old_cursor() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime.snapshot.last_verified_unix_seconds = Some(1);
    runtime.snapshot.local_history_backfill_cursor = Some("stale-page".to_owned());
    runtime.snapshot.local_history_backfill_completed = true;

    runtime
        .prepare_local_history_backfill(
            1 + crate::state::history_repository::replica::runtime::TOMBSTONE_HORIZON_SECONDS + 1,
        )
        .expect("rebuild stale local history before reading a page cursor");

    assert_eq!(runtime.local_history_backfill_cursor(), None);
    assert!(!runtime.local_history_backfill_completed());
}

#[test]
fn stale_rebuild_preserves_unacknowledged_backfill_wire_across_restart() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let queued = runtime
        .queue_local_history_backfill_batches(
            "cluster-a",
            identity,
            &key,
            Some("page".to_owned()),
            false,
            vec![(vec![record(b"pending", false)], 1_000)],
        )
        .expect("queue signed page");
    runtime.snapshot.last_verified_unix_seconds = Some(1);
    runtime
        .persist_control_state()
        .expect("persist stale state");
    let now = 2 + crate::state::history_repository::replica::runtime::TOMBSTONE_HORIZON_SECONDS;
    assert!(runtime.prepare_local_history_backfill(now).is_err());
    drop(runtime);
    let mut restored = load(temporary.path());
    assert!(restored.prepare_local_history_backfill(now).is_err());
    let pending = restored
        .local_history_backfill_inflight_segments()
        .expect("pending page");
    assert_eq!(pending[0].wire, queued[0].wire);
    assert_eq!(
        restored.local_history_backfill_inflight_checkpoint(),
        Some((Some("page".to_owned()), false))
    );
}

#[test]
fn collected_backfill_page_is_rejected_if_rebuild_changes_state() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime.snapshot.last_verified_unix_seconds = Some(1);
    runtime.snapshot.local_history_backfill_cursor = Some("old-page".to_owned());
    let collection_state = runtime.local_history_backfill_collection_state();
    runtime
        .prepare_local_history_backfill(
            2 + crate::state::history_repository::replica::runtime::TOMBSTONE_HORIZON_SECONDS,
        )
        .expect("rebuild");
    assert!(
        runtime
            .validate_local_history_backfill_collection_state(&collection_state)
            .is_err()
    );
    assert_eq!(runtime.local_history_backfill_cursor(), None);
}
