use super::super::{RepositoryNodeIdentity, RepositoryReplicaRuntime, RepositoryReplicaSegment};
use super::*;

const FIRST_PAGE_CREATED_AT: u64 = HORIZON + 10_000;
const HORIZON: u64 = crate::state::history_repository::replica::runtime::TOMBSTONE_HORIZON_SECONDS;

fn queue_first_pending_page(
    runtime: &mut RepositoryReplicaRuntime,
    created_at: u64,
) -> Vec<RepositoryReplicaSegment> {
    let key = signing_key();
    // Historical observation time must never stand in for the queue-time freshness anchor.
    runtime
        .queue_local_history_backfill_batches_at(
            "cluster-a",
            identity(&key),
            &key,
            Some("first-page".to_owned()),
            true,
            vec![(vec![record(b"first-pending", false)], created_at - 1)],
            created_at,
        )
        .expect("persist first pending page at explicit queue time")
}

fn assert_pending_replay_rejected_without_mutation(
    runtime: &mut RepositoryReplicaRuntime,
    source_identity: &RepositoryNodeIdentity,
    now: u64,
) {
    let snapshot = serde_json::to_value(&runtime.snapshot).expect("snapshot before replay");
    let tombstones = serde_json::to_value(runtime.tombstones.checkpoint()).expect("tombstones");
    let receiver = runtime
        .receiver
        .as_ref()
        .map(|receiver| receiver.checkpoint().expect("receiver checkpoint"));
    let receiver = serde_json::to_value(receiver).expect("receiver before replay");
    let count = runtime
        .storage
        .repository_history_record_count()
        .expect("count");
    let pending = runtime
        .local_history_backfill_inflight_segments()
        .expect("pending wire");
    assert_eq!(pending.len(), 1);
    for path in 0..3 {
        let result = match path {
            0 => runtime.apply_local_history_backfill_inflight_page(
                "cluster-a",
                source_identity,
                now,
                &["local".to_owned()],
                "local",
            ),
            1 => runtime
                .receive_local_history_backfill_wire(
                    "cluster-a",
                    source_identity,
                    &pending[0].wire,
                    now,
                    &["local".to_owned()],
                    "local",
                )
                .map(|_| ()),
            _ => runtime
                .receive_local_history_backfill_legacy_page(
                    "cluster-a",
                    &pending,
                    now,
                    &["local".to_owned()],
                    "local",
                )
                .map(|_| ()),
        };
        let error = result.expect_err("unsafe pending page must fail closed on every replay path");
        assert!(error.to_string().contains("stale"), "{error}");
        assert_eq!(
            serde_json::to_value(&runtime.snapshot).expect("snapshot"),
            snapshot
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
                    .map(|receiver| { receiver.checkpoint().expect("receiver checkpoint") })
            )
            .expect("receiver after replay"),
            receiver
        );
        assert_eq!(
            runtime
                .storage
                .repository_history_record_count()
                .expect("count"),
            count
        );
    }
}

#[test]
fn first_pending_page_replays_from_queue_time_after_restart() {
    for elapsed in [0, HORIZON / 2, HORIZON] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let key = signing_key();
        let source_identity = identity(&key);
        let mut runtime = load(temporary.path());
        let pending = queue_first_pending_page(&mut runtime, FIRST_PAGE_CREATED_AT);
        assert_eq!(runtime.snapshot.last_verified_unix_seconds, None);
        assert_eq!(runtime.local_history_backfill_cursor(), None);
        drop(runtime);

        let mut restored = load(temporary.path());
        let snapshot = serde_json::to_value(&restored.snapshot).expect("persisted snapshot");
        assert_eq!(
            snapshot["local_history_backfill_inflight"]["created_at_unix_seconds"],
            FIRST_PAGE_CREATED_AT
        );
        assert_eq!(restored.snapshot.last_verified_unix_seconds, None);
        let wire = restored
            .local_history_backfill_inflight_segments()
            .expect("pending");
        assert_eq!(wire[0].wire, pending[0].wire);
        let signed = SignedSegment::from_wire(&wire[0].wire).expect("signed wire");
        signed
            .verify(&key.verifying_key())
            .expect("original signature");
        assert_eq!(
            signed.canonical().closed_at_unix_seconds(),
            FIRST_PAGE_CREATED_AT - 1
        );
        assert_eq!(signed.canonical().first_cursor().sequence(), 0);
        let now = FIRST_PAGE_CREATED_AT + elapsed;
        restored
            .apply_local_history_backfill_inflight_page(
                "cluster-a",
                &source_identity,
                now,
                &["local".to_owned()],
                "local",
            )
            .expect("first pending page is fresh through the inclusive queue-time horizon");
        drop(restored);

        let restored = load(temporary.path());
        assert_eq!(restored.local_history_backfill_cursor(), Some("first-page"));
        assert!(restored.local_history_backfill_completed());
        assert!(
            restored
                .local_history_backfill_inflight_checkpoint()
                .is_none()
        );
        assert_eq!(restored.snapshot.last_verified_unix_seconds, Some(now));
        assert_eq!(
            restored
                .snapshot
                .local_history_backfill_stream_state
                .get(signed.canonical().first_cursor().stream()),
            Some(&(1, Some(signed.segment_hash().expect("signed hash"))))
        );
        // Later replays may legitimately prune historical rows after the atomic import.
        if elapsed == 0 {
            assert_eq!(
                restored
                    .storage
                    .repository_history_record_count()
                    .expect("count"),
                1
            );
            let stored = stored_segments(&restored);
            assert_eq!(stored.len(), 1);
            assert_eq!(
                stored[0].wire_bytes().expect("accepted wire"),
                pending[0].wire
            );
        }
    }
}

#[test]
fn first_pending_page_expiry_or_clock_regression_cannot_establish_freshness() {
    let expired = FIRST_PAGE_CREATED_AT + HORIZON + 1;
    for (now, last_verified) in [
        (expired, None),
        (FIRST_PAGE_CREATED_AT - 1, None),
        // Another receive can refresh the repository without making this old page safe.
        (expired, Some(expired)),
        // A fresh pending page cannot override an expired verification anchor.
        (
            FIRST_PAGE_CREATED_AT,
            Some(FIRST_PAGE_CREATED_AT - HORIZON - 1),
        ),
        (FIRST_PAGE_CREATED_AT, Some(FIRST_PAGE_CREATED_AT + 1)),
    ] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let key = signing_key();
        let source_identity = identity(&key);
        let mut runtime = load(temporary.path());
        let pending = queue_first_pending_page(&mut runtime, FIRST_PAGE_CREATED_AT);
        runtime.snapshot.last_verified_unix_seconds = last_verified;
        runtime
            .persist_control_state()
            .expect("persist verification anchor");
        drop(runtime);

        for _ in 0..2 {
            let mut restored = load(temporary.path());
            assert_eq!(restored.snapshot.last_verified_unix_seconds, last_verified);
            assert_eq!(restored.local_history_backfill_cursor(), None);
            assert!(!restored.local_history_backfill_completed());
            assert_eq!(
                restored.local_history_backfill_inflight_checkpoint(),
                Some((Some("first-page".to_owned()), true))
            );
            assert_eq!(
                restored
                    .local_history_backfill_inflight_segments()
                    .expect("pending")[0]
                    .wire,
                pending[0].wire
            );
            assert_pending_replay_rejected_without_mutation(&mut restored, &source_identity, now);
        }
    }
}

#[test]
fn legacy_first_pending_page_without_creation_time_fails_closed_after_restart() {
    for last_verified in [None, Some(FIRST_PAGE_CREATED_AT)] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let key = signing_key();
        let source_identity = identity(&key);
        let mut runtime = load(temporary.path());
        let pending = queue_first_pending_page(&mut runtime, FIRST_PAGE_CREATED_AT);
        runtime.snapshot.last_verified_unix_seconds = last_verified;
        let mut legacy =
            serde_json::to_value(runtime.snapshot_for_persistence()).expect("snapshot");
        assert!(
            legacy["local_history_backfill_inflight"]
                .as_object_mut()
                .expect("pending page")
                .remove("created_at_unix_seconds")
                .is_some()
        );
        // Write an actually absent field, exercising predecessor-format deserialization at load.
        runtime
            .storage
            .write(
                crate::state::history_storage::REPOSITORY_REPLICA_KEY,
                &serde_json::to_vec(&legacy).expect("legacy JSON"),
            )
            .expect("persist predecessor pending page");
        drop(runtime);

        for _ in 0..2 {
            let mut restored = load(temporary.path());
            assert_eq!(restored.snapshot.last_verified_unix_seconds, last_verified);
            assert_eq!(restored.local_history_backfill_cursor(), None);
            assert!(!restored.local_history_backfill_completed());
            assert_eq!(
                restored
                    .local_history_backfill_inflight_segments()
                    .expect("legacy wire")[0]
                    .wire,
                pending[0].wire
            );
            assert_pending_replay_rejected_without_mutation(
                &mut restored,
                &source_identity,
                FIRST_PAGE_CREATED_AT,
            );
        }
    }
}

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
        .queue_local_history_backfill_batches_at(
            "cluster-a",
            identity.clone(),
            &key,
            None,
            true,
            vec![(vec![record(b"boundary", false)], 1_000)],
            1_001,
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
