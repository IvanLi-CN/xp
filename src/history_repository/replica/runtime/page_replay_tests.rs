use super::tests::{identity, load, record, signing_key};
use crate::history_sync::{ProtocolError, SignedSegment, SyncRecord};
use sha2::{Digest as _, Sha256};

fn stored_segments(runtime: &super::RepositoryReplicaRuntime) -> Vec<SignedSegment> {
    let ids = runtime
        .replication_summary()
        .expect("replication summary")
        .segment_ids;
    runtime
        .repair_batch(&ids)
        .expect("repair batch")
        .segments
        .into_iter()
        .map(|segment| SignedSegment::from_wire(&segment.wire).expect("signed segment"))
        .collect()
}

#[test]
fn live_source_queue_keeps_live_stream_and_ack_path_intact() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let repository_identity = identity(&key);
    let mut runtime = load(temporary.path());
    let segment = runtime
        .queue_local_source_segment(
            "cluster-a",
            repository_identity.clone(),
            &key,
            vec![record(b"live-source", false)],
            900,
        )
        .expect("queue live source segment")
        .expect("live source segment");
    let signed = SignedSegment::from_wire(&segment.wire).expect("signed live segment");
    assert_eq!(signed.canonical().first_cursor().stream(), "runtime");
    assert_eq!(signed.canonical().first_cursor().sequence(), 0);
    assert_eq!(
        runtime
            .storage
            .source_delivery_journal()
            .expect("source journal after queue")
            .len(),
        1
    );

    runtime
        .receive_wire("cluster-a", &repository_identity, &segment.wire, 901)
        .expect("receive live source segment");
    runtime
        .acknowledge_local_source_segment(&segment.wire)
        .expect("acknowledge live source segment");
    assert!(runtime.local_source_pending_segments().is_empty());
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .expect("source journal after ack")
            .is_empty()
    );
}

#[test]
fn local_history_backfill_starts_a_separate_chain_without_touching_live_source_state() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let repository_identity = identity(&key);
    let mut runtime = load(temporary.path());
    let live = runtime
        .queue_local_source_segment(
            "cluster-a",
            repository_identity.clone(),
            &key,
            vec![record(b"live", false)],
            900,
        )
        .expect("queue live source segment")
        .expect("live source segment");
    runtime
        .queue_local_source_segment(
            "cluster-a",
            repository_identity.clone(),
            &key,
            vec![record(b"live-tail", false)],
            901,
        )
        .expect("queue live tail segment")
        .expect("live tail segment");
    let source_journal_before = runtime
        .storage
        .source_delivery_journal()
        .expect("source journal before backfill");
    let live_tail_wire = source_journal_before[1].wire.clone();

    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            repository_identity,
            &key,
            Some("history-page-1".to_owned()),
            false,
            vec![(vec![record(b"historical", false)], 1_000)],
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect("apply local backfill page");

    let backfill = stored_segments(&runtime);
    assert_eq!(backfill.len(), 1);
    assert_eq!(backfill[0].canonical().first_cursor().sequence(), 0);
    assert_eq!(
        backfill[0].canonical().first_cursor().stream(),
        "runtime-backfill-node-a"
    );
    assert_eq!(runtime.local_source_next_sequence("runtime"), Some(2));
    assert_eq!(
        SignedSegment::from_wire(&live_tail_wire)
            .expect("signed live tail segment")
            .canonical()
            .first_cursor()
            .sequence(),
        1
    );
    let pending_journal = runtime
        .storage
        .source_delivery_journal()
        .expect("source journal pending after backfill");
    assert_eq!(pending_journal.len(), 2);
    assert_eq!(pending_journal[0].wire, live.wire);
    assert_eq!(pending_journal[1].wire, live_tail_wire);
    let source_journal_after = runtime
        .storage
        .source_delivery_journal()
        .expect("source journal after backfill");
    assert_eq!(source_journal_after.len(), source_journal_before.len());
    assert_eq!(source_journal_after[0].wire, source_journal_before[0].wire);
    assert_eq!(
        runtime.local_history_backfill_cursor(),
        Some("history-page-1")
    );

    runtime
        .receive_wire("cluster-a", &identity(&key), &live.wire, 1_002)
        .expect("receive non-zero live source segment");
    runtime
        .acknowledge_local_source_segment(&live.wire)
        .expect("acknowledge non-zero live source segment");
    runtime
        .receive_wire("cluster-a", &identity(&key), &live_tail_wire, 1_003)
        .expect("receive live tail segment");
    runtime
        .acknowledge_local_source_segment(&live_tail_wire)
        .expect("acknowledge live tail segment");
    assert!(runtime.local_source_pending_segments().is_empty());
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .expect("source journal after live replay")
            .is_empty()
    );
}

#[test]
fn malformed_local_history_backfill_inflight_index_cannot_advance_checkpoint() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let repository_identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .queue_local_history_backfill_segments(
            "cluster-a",
            repository_identity.clone(),
            &key,
            vec![record(b"malformed-inflight", false)],
            1_000,
        )
        .expect("queue local backfill segment");
    let mut snapshot = serde_json::to_value(&runtime.snapshot).expect("serialize snapshot");
    snapshot["local_history_backfill_inflight"]["segment_ids"] = serde_json::json!([]);
    runtime.snapshot = serde_json::from_value(snapshot).expect("restore malformed snapshot");

    let error = runtime
        .apply_local_history_backfill_inflight_page(
            "cluster-a",
            &repository_identity,
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect_err("malformed in-flight index must fail closed");
    assert!(matches!(
        error,
        super::RepositoryRuntimeError::Storage(message)
            if message.contains("in-flight segment index")
    ));
    assert!(runtime.snapshot.local_history_backfill_inflight.is_some());
    assert_eq!(runtime.local_history_backfill_cursor(), None);
}

#[test]
fn local_history_backfill_reuses_the_durable_live_epoch_after_source_queue_rollback() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime.set_local_source_epoch(0, "");
    let initial_epoch = super::source_epoch("cluster-a", identity.node_id().as_str());
    runtime
        .storage
        .allocate_repository_source_epoch("cluster-a", identity.node_id().as_str(), initial_epoch)
        .expect("persist source epoch allocation");

    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity.clone(),
            &key,
            Some("history-page-1".to_owned()),
            true,
            vec![(vec![record(b"historical-after-rollback", false)], 1_000)],
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect("apply local backfill page");

    let backfill = stored_segments(&runtime);
    assert_eq!(backfill.len(), 1);
    assert_eq!(
        backfill[0].canonical().first_cursor().source_epoch(),
        initial_epoch + 1
    );
    let live = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity,
            &key,
            vec![record(b"live-after-backfill", false)],
            1_002,
        )
        .expect("queue live source segment")
        .expect("live source segment");
    assert_eq!(
        SignedSegment::from_wire(&live.wire)
            .expect("signed live segment")
            .canonical()
            .first_cursor()
            .source_epoch(),
        initial_epoch + 1
    );
}

#[test]
fn local_history_backfill_restart_continues_its_persisted_hash_chain() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity.clone(),
            &key,
            Some("history-page-1".to_owned()),
            false,
            vec![(vec![record(b"historical-1", false)], 1_000)],
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect("apply first page");
    drop(runtime);

    let mut restarted = load(temporary.path());
    restarted
        .apply_local_history_backfill_page(
            "cluster-a",
            identity,
            &key,
            Some("history-page-2".to_owned()),
            true,
            vec![(vec![record(b"historical-2", false)], 1_002)],
            1_003,
            &["local".to_owned()],
            "local",
        )
        .expect("apply second page after restart");

    let mut segments = stored_segments(&restarted);
    segments.sort_by_key(|segment| segment.canonical().first_cursor().sequence());
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].canonical().first_cursor().sequence(), 0);
    assert_eq!(segments[1].canonical().first_cursor().sequence(), 1);
    assert_eq!(
        segments[1].canonical().records()[0].record_key(),
        b"historical-2"
    );
    assert_eq!(
        restarted.local_history_backfill_cursor(),
        Some("history-page-2")
    );
    assert!(restarted.local_history_backfill_completed());
}

#[test]
fn local_history_backfill_rejects_a_predecessor_hash_conflict_before_apply() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity.clone(),
            &key,
            Some("history-page-1".to_owned()),
            false,
            vec![(vec![record(b"historical-1", false)], 1_000)],
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect("apply first page");
    runtime
        .queue_local_history_backfill_segments(
            "cluster-a",
            identity.clone(),
            &key,
            vec![record(b"historical-2", false)],
            1_002,
        )
        .expect("queue second page");
    runtime
        .snapshot
        .local_history_backfill_stream_state
        .insert("runtime-backfill-node-a".to_owned(), (1, Some([7; 32])));

    let error = runtime
        .apply_local_history_backfill_inflight_page(
            "cluster-a",
            &identity,
            1_003,
            &["local".to_owned()],
            "local",
        )
        .expect_err("predecessor hash conflict must fail closed");
    assert!(matches!(
        error,
        super::RepositoryRuntimeError::Protocol(ProtocolError::HashChainMismatch)
    ));
    assert!(
        runtime
            .local_history_backfill_inflight_checkpoint()
            .is_some()
    );
    assert_eq!(
        runtime.local_history_backfill_cursor(),
        Some("history-page-1")
    );
}

#[test]
fn local_history_backfill_recovery_does_not_rebuild_away_persisted_wires() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .queue_local_history_backfill_segments(
            "cluster-a",
            identity.clone(),
            &key,
            vec![record(b"stale-recovery", false)],
            1_000,
        )
        .expect("queue persisted page");
    runtime.snapshot.last_verified_unix_seconds = Some(1);

    runtime
        .apply_local_history_backfill_inflight_page(
            "cluster-a",
            &identity,
            1 + super::TOMBSTONE_HORIZON_SECONDS + 1,
            &["local".to_owned()],
            "local",
        )
        .expect("replay persisted page without rebuilding it away");
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("history count"),
        1
    );
}

#[test]
fn local_history_backfill_commit_failure_rolls_back_records_and_cursor() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .storage
        .set_query_only_for_test(true)
        .expect("enable SQLite write failure");

    assert!(
        runtime
            .apply_local_history_backfill_page(
                "cluster-a",
                identity.clone(),
                &key,
                Some("history-page-1".to_owned()),
                true,
                vec![(vec![record(b"atomic", false)], 1_000)],
                1_001,
                &["local".to_owned()],
                "local",
            )
            .is_err()
    );
    assert_eq!(runtime.local_history_backfill_cursor(), None);
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("history count after rollback"),
        0
    );
    assert!(runtime.local_source_pending_segments().is_empty());

    runtime
        .storage
        .set_query_only_for_test(false)
        .expect("disable SQLite write failure");
    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity,
            &key,
            Some("history-page-1".to_owned()),
            true,
            vec![(vec![record(b"atomic", false)], 1_000)],
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect("retry atomic page");
    assert_eq!(
        runtime.local_history_backfill_cursor(),
        Some("history-page-1")
    );
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("history count after retry"),
        1
    );
}

#[test]
fn local_history_backfill_persists_exact_wire_before_apply_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .queue_local_history_backfill_batches(
            "cluster-a",
            identity.clone(),
            &key,
            Some("history-page-1".to_owned()),
            true,
            vec![(vec![record(b"retry-wire", false)], 1_000)],
        )
        .expect("persist backfill wire");
    let persisted = runtime
        .local_history_backfill_inflight_segments()
        .expect("read persisted wire");
    assert_eq!(persisted.len(), 1);

    runtime
        .storage
        .set_query_only_for_test(true)
        .expect("enable SQLite write failure");
    assert!(
        runtime
            .apply_local_history_backfill_inflight_page(
                "cluster-a",
                &identity,
                1_001,
                &["local".to_owned()],
                "local",
            )
            .is_err()
    );
    let retry = runtime
        .local_history_backfill_inflight_segments()
        .expect("retain persisted wire after failure");
    assert_eq!(retry[0].wire, persisted[0].wire);
    assert_eq!(runtime.local_history_backfill_cursor(), None);
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("history count after failed apply"),
        0
    );

    runtime
        .storage
        .set_query_only_for_test(false)
        .expect("disable SQLite write failure");
    runtime
        .apply_local_history_backfill_inflight_page(
            "cluster-a",
            &identity,
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect("retry persisted backfill wire");
    assert!(
        runtime
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    assert_eq!(
        runtime.local_history_backfill_cursor(),
        Some("history-page-1")
    );
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("history count after retry"),
        1
    );
}

#[test]
fn local_history_backfill_tombstone_protects_the_independent_record_stream() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let historical_key = b"node-history:node:node-a:record".to_vec();
    let tombstone_key = b"node-history:node:node-a:".to_vec();
    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity.clone(),
            &key,
            Some("history-page-1".to_owned()),
            false,
            vec![(
                vec![SyncRecord::new(
                    "subject-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    historical_key.clone(),
                    b"historical".to_vec(),
                    false,
                )],
                1_000,
            )],
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect("apply historical record");
    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity.clone(),
            &key,
            Some("history-page-2".to_owned()),
            false,
            vec![(
                vec![SyncRecord::new(
                    "subject-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    tombstone_key,
                    Vec::new(),
                    true,
                )],
                1_001,
            )],
            1_002,
            &["local".to_owned()],
            "local",
        )
        .expect("apply historical tombstone");

    let error = runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity,
            &key,
            Some("history-page-3".to_owned()),
            true,
            vec![(
                vec![SyncRecord::new(
                    "subject-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    historical_key,
                    b"resurrection".to_vec(),
                    false,
                )],
                1_002,
            )],
            1_003,
            &["local".to_owned()],
            "local",
        )
        .expect_err("tombstone must prevent resurrection");
    assert!(matches!(
        error,
        super::RepositoryRuntimeError::Protocol(ProtocolError::ResurrectionPrevented)
    ));
    assert_eq!(
        runtime.local_history_backfill_cursor(),
        Some("history-page-2")
    );
}

#[test]
fn local_history_backfill_exact_tombstone_removes_the_independent_record() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let historical_key = b"exact-history-key".to_vec();
    let mut runtime = load(temporary.path());
    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity.clone(),
            &key,
            Some("history-page-1".to_owned()),
            false,
            vec![(
                vec![SyncRecord::new(
                    "subject-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    historical_key.clone(),
                    b"historical".to_vec(),
                    false,
                )],
                1_000,
            )],
            1_001,
            &["local".to_owned()],
            "local",
        )
        .expect("apply historical record");
    assert_eq!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 8)
            .expect("read historical record")
            .len(),
        1
    );

    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity,
            &key,
            Some("history-page-2".to_owned()),
            true,
            vec![(
                vec![SyncRecord::new(
                    "subject-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    historical_key,
                    Vec::new(),
                    true,
                )],
                1_001,
            )],
            1_002,
            &["local".to_owned()],
            "local",
        )
        .expect("apply exact tombstone");
    assert!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 8)
            .expect("read records after exact tombstone")
            .is_empty()
    );
}

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

#[test]
fn predecessor_local_history_backfill_page_replays_source_journal_wire() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let queued = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity.clone(),
            &key,
            vec![record(b"predecessor-page", false)],
            1_000,
        )
        .expect("queue predecessor page")
        .expect("predecessor segment");
    let segment_id = hex::encode(Sha256::digest(&queued.wire));
    let mut snapshot = serde_json::to_value(&runtime.snapshot).expect("serialize snapshot");
    snapshot["local_history_backfill_inflight"] = serde_json::json!({
        "page_cursor": "history-page-legacy",
        "completed": false,
        "segment_ids": [segment_id],
    });
    runtime
        .storage
        .write(
            crate::state::history_storage::REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).expect("serialize predecessor snapshot"),
        )
        .expect("persist predecessor snapshot");
    drop(runtime);

    let mut restored = load(temporary.path());
    let replay = restored
        .local_history_backfill_inflight_segments()
        .expect("load predecessor page");
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].wire, queued.wire);
    restored
        .receive_wire("cluster-a", &replay[0].identity, &replay[0].wire, 1_001)
        .expect("replay predecessor segment");
    restored
        .storage
        .set_query_only_for_test(true)
        .expect("enable acknowledgement failure");
    assert!(
        restored
            .acknowledge_local_source_segments_and_checkpoint_backfill(
                &replay,
                Some("history-page-legacy".to_owned()),
                false,
            )
            .is_err()
    );
    assert!(
        restored
            .local_history_backfill_inflight_checkpoint()
            .is_some()
    );
    assert_eq!(restored.local_history_backfill_cursor(), None);
    assert_eq!(
        restored
            .storage
            .source_delivery_journal()
            .expect("journal after failed acknowledgement")
            .len(),
        1
    );
    restored
        .storage
        .set_query_only_for_test(false)
        .expect("disable acknowledgement failure");
    restored
        .acknowledge_local_source_segments_and_checkpoint_backfill(
            &replay,
            Some("history-page-legacy".to_owned()),
            false,
        )
        .expect("acknowledge predecessor page");

    let restored = load(temporary.path());
    assert!(restored.local_source_pending_segments().is_empty());
    assert!(
        restored
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    assert_eq!(
        restored.local_history_backfill_cursor(),
        Some("history-page-legacy")
    );
}

#[test]
fn legacy_local_history_backfill_page_applies_all_segments_atomically() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let first = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity.clone(),
            &key,
            vec![record(b"legacy-page-1", false)],
            1_000,
        )
        .expect("queue first legacy segment")
        .expect("first legacy segment");
    let mut second = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity.clone(),
            &key,
            vec![record(b"legacy-page-2", false)],
            1_001,
        )
        .expect("queue second legacy segment")
        .expect("second legacy segment");
    second.wire[0] ^= 1;

    assert!(
        runtime
            .receive_local_history_backfill_legacy_page(
                "cluster-a",
                &[first, second],
                1_002,
                &["local".to_owned()],
                "local",
            )
            .is_err()
    );
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("history count after failed page"),
        0,
        "a failed legacy page must not leave an earlier segment applied"
    );
}

#[test]
fn source_delivery_ack_defers_a_legacy_backfill_owned_wire() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let queued = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity,
            &key,
            vec![record(b"legacy-owned", false)],
            1_000,
        )
        .expect("queue legacy segment")
        .expect("legacy segment");
    let segment_id = hex::encode(Sha256::digest(&queued.wire));
    let mut snapshot = serde_json::to_value(&runtime.snapshot).expect("serialize snapshot");
    snapshot["local_history_backfill_inflight"] = serde_json::json!({
        "page_cursor": "history-page-legacy",
        "completed": false,
        "segment_ids": [segment_id],
    });
    runtime
        .storage
        .write(
            crate::state::history_storage::REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).expect("serialize legacy snapshot"),
        )
        .expect("persist legacy snapshot");
    let mut restored = load(temporary.path());

    restored
        .acknowledge_local_source_segment(&queued.wire)
        .expect("defer source acknowledgement");
    assert_eq!(
        restored
            .storage
            .source_delivery_journal()
            .expect("read source journal")
            .len(),
        1
    );
    assert_eq!(restored.local_source_pending_segments().len(), 1);
}
