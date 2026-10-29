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

#[test]
fn stale_pending_page_cannot_refresh_receiver_through_direct_replay() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity.clone(),
            &key,
            Some("accepted".to_owned()),
            false,
            vec![
                (vec![record(b"prefix", false)], 1_000),
                (vec![record(b"deleted", true)], 1_000),
            ],
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect("accept chain prefix and deletion evidence");
    let pending = runtime
        .queue_local_history_backfill_batches(
            "cluster-a",
            identity.clone(),
            &key,
            Some("pending".to_owned()),
            true,
            vec![(vec![record(b"tail", false)], 1_002)],
        )
        .expect("persist nonzero pending chain tail");
    let signed = SignedSegment::from_wire(&pending[0].wire).expect("signed pending wire");
    assert_eq!(signed.canonical().first_cursor().sequence(), 1);
    assert!(signed.canonical().previous_segment_hash().is_some());
    runtime.snapshot.gaps.push(super::super::StoredGap {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_sequence: 0,
        last_sequence: 1,
        start_unix_seconds: 1_000,
        end_unix_seconds: 1_002,
        permanent: true,
        reason: Some("source_retention_expired".to_owned()),
    });
    runtime
        .persist_control_state()
        .expect("persist permanent gap");
    let now = 1_002 + crate::state::history_repository::replica::runtime::TOMBSTONE_HORIZON_SECONDS;
    let before = serde_json::to_value(&runtime.snapshot).expect("snapshot before replay");
    let tombstones = serde_json::to_value(runtime.tombstones.checkpoint()).expect("tombstones");
    let receiver = runtime
        .receiver
        .as_ref()
        .expect("receiver")
        .checkpoint()
        .expect("checkpoint");
    let receiver = serde_json::to_value(receiver).expect("receiver state");
    for path in 0..4 {
        let error = match path {
            0 => runtime.apply_local_history_backfill_inflight_page(
                "cluster-a",
                &identity,
                now,
                &["local".to_owned()],
                "local",
            ),
            1 => runtime
                .receive_local_history_backfill_wire(
                    "cluster-a",
                    &identity,
                    &pending[0].wire,
                    now,
                    &["local".to_owned()],
                    "local",
                )
                .map(|_| ()),
            2 => runtime
                .receive_local_history_backfill_legacy_page(
                    "cluster-a",
                    &pending,
                    now,
                    &["local".to_owned()],
                    "local",
                )
                .map(|_| ()),
            _ => runtime.prepare_local_history_backfill(now),
        }
        .expect_err("stale pending replay must fail closed");
        assert!(error.to_string().contains("stale"));
        assert_eq!(
            serde_json::to_value(&runtime.snapshot).expect("snapshot"),
            before
        );
        assert_eq!(
            serde_json::to_value(runtime.tombstones.checkpoint()).expect("tombstones"),
            tombstones
        );
        assert_eq!(
            serde_json::to_value(
                runtime
                    .receiver
                    .as_ref()
                    .expect("receiver")
                    .checkpoint()
                    .expect("checkpoint")
            )
            .expect("receiver state"),
            receiver
        );
    }
    let count = runtime
        .storage
        .repository_history_record_count()
        .expect("record count");
    drop(runtime);
    let restored = load(temporary.path());
    assert_eq!(restored.snapshot.last_verified_unix_seconds, Some(1_001));
    assert!(restored.snapshot.gaps.iter().any(|gap| gap.permanent));
    let response = restored
        .query(
            "local",
            super::super::HistoryQuery::new(1_000, 1_002, 10).expect("query"),
            super::super::LocalQueryMetadata::current_window(now),
        )
        .expect("query with permanent gap after rejected replay");
    assert_eq!(
        response.plan().completeness(),
        crate::state::history_repository::query::Completeness::Partial
    );
    assert_eq!(restored.local_history_backfill_cursor(), Some("accepted"));
    assert!(!restored.local_history_backfill_completed());
    assert_eq!(
        restored
            .local_history_backfill_inflight_segments()
            .expect("pending")[0]
            .wire,
        pending[0].wire
    );
    assert_eq!(
        restored
            .storage
            .repository_history_record_count()
            .expect("durable count"),
        count
    );
}

#[test]
fn pending_page_replay_at_tombstone_horizon_remains_valid() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .queue_local_history_backfill_batches(
            "cluster-a",
            identity.clone(),
            &key,
            None,
            true,
            vec![(vec![record(b"boundary", false)], 1_000)],
        )
        .expect("queue page");
    runtime.snapshot.last_verified_unix_seconds = Some(1_001);
    let now = 1_001 + crate::state::history_repository::replica::runtime::TOMBSTONE_HORIZON_SECONDS;
    runtime
        .apply_local_history_backfill_inflight_page(
            "cluster-a",
            &identity,
            now,
            &["local".to_owned()],
            "local",
        )
        .expect("exact horizon is still fresh");
    assert!(runtime.local_history_backfill_completed());
    assert!(
        runtime
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    assert_eq!(runtime.snapshot.last_verified_unix_seconds, Some(now));
}

#[test]
fn page_collected_before_expiry_cannot_be_queued_after_expiry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime.snapshot.last_verified_unix_seconds = Some(1_001);
    runtime.persist_control_state().expect("persist freshness");
    let before = serde_json::to_value(&runtime.snapshot).expect("snapshot");
    let now = 1_002 + crate::state::history_repository::replica::runtime::TOMBSTONE_HORIZON_SECONDS;
    let error = runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity,
            &key,
            None,
            true,
            vec![(vec![record(b"collected-before-expiry", false)], 1_000)],
            now,
            &["local".to_owned()],
            "local",
        )
        .expect_err("expiry during collection requires another preparation");
    assert!(error.to_string().contains("tombstone-safe rebuild"));
    assert_eq!(
        serde_json::to_value(&runtime.snapshot).expect("snapshot"),
        before
    );
    assert!(
        runtime
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("count"),
        0
    );
    let restored = load(temporary.path());
    assert_eq!(restored.snapshot.last_verified_unix_seconds, Some(1_001));
    assert!(
        restored
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
}
