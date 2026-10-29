use super::tests::{identity, load, record, signing_key};
use crate::history_sync::{ProtocolError, SignedSegment, SyncRecord};

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
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let segment = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity.clone(),
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
        .receive_wire("cluster-a", &identity, &segment.wire, 901)
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
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let live = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity.clone(),
            &key,
            vec![record(b"live", false)],
            900,
        )
        .expect("queue live source segment")
        .expect("live source segment");
    let source_journal_before = runtime
        .storage
        .source_delivery_journal()
        .expect("source journal before backfill");

    runtime
        .apply_local_history_backfill_page(
            "cluster-a",
            identity,
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
    assert_eq!(runtime.local_source_next_sequence("runtime"), Some(1));
    assert_eq!(runtime.local_source_pending_segments()[0].wire, live.wire);
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
