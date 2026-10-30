use super::*;

fn tombstone(sequence: u64, key: &[u8], previous: Option<[u8; 32]>) -> SignedSegment {
    CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "tombstone", sequence).unwrap(),
        vec![record(key, true)],
        previous,
        11,
        11,
    )
    .unwrap()
    .sign(&signing_key())
    .unwrap()
}

pub(super) fn journal_row(segment: &SignedSegment) -> SourceDeliveryJournalRow {
    let wire = segment.wire_bytes().unwrap();
    SourceDeliveryJournalRow {
        id: hex::encode(Sha256::digest(&wire)),
        stream: segment.canonical().first_cursor().stream().into(),
        closed_at_unix_seconds: 11,
        identity: identity(&signing_key()),
        wire,
    }
}

fn setup_page(
    runtime: &mut RepositoryReplicaRuntime,
    tombstones: &[SignedSegment],
) -> Vec<SignedSegment> {
    let ordinary = prepare(runtime, 3);
    let added = [
        journal_row(&ordinary[2]),
        journal_row(&tombstones[1]),
        journal_row(&tombstones[2]),
    ];
    runtime
        .storage
        .append_source_delivery_journal(&added)
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["tombstone"] = serde_json::json!({
        "next_sequence": 3,
        "previous_segment_hash": tombstones[2].segment_hash().unwrap(),
        "pending": []
    });
    snapshot["local_history_backfill_inflight"]["segment_ids"] = serde_json::json!([
        added[0].id,
        added[1].id,
        journal_row(&ordinary[3]).id,
        added[2].id
    ]);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    ordinary
}

pub(super) fn stage(
    runtime: &mut RepositoryReplicaRuntime,
    state: &LegacyRecoveryState,
    segments: &[SignedSegment],
) -> LegacyRecoveryState {
    let mut request = state.clone();
    request.pending = segments
        .iter()
        .map(|segment| journal_row(segment).id)
        .collect();
    runtime
        .legacy_recovery_update_scan(state, &request)
        .unwrap();
    runtime
        .legacy_recovery_stage_response(
            "cluster-a",
            &identity(&signing_key()),
            &request,
            &response(segments),
        )
        .unwrap()
}

#[test]
fn mixed_stream_page_applies_tombstone_prefix_before_records_and_rolls_back_ack() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let first = tombstone(0, b"0", None);
    let second = tombstone(1, b"1", Some(first.segment_hash().unwrap()));
    let third = tombstone(2, b"2", Some(second.segment_hash().unwrap()));
    let tombstones = vec![first, second, third];
    let ordinary = setup_page(&mut runtime, &tombstones);
    let source = identity(&signing_key());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    // The first tombstone is not in the local journal and must come from signed repair.
    let state = stage(
        &mut runtime,
        &state,
        &[
            ordinary[0].clone(),
            ordinary[1].clone(),
            tombstones[0].clone(),
        ],
    );
    assert_eq!(state.targets["tombstone"].next, 1);
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.targets["tombstone"].next, 2);
    drop(runtime);
    runtime = load(directory.path());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.targets["tombstone"].next, 3);
    assert_eq!(state.phase, "apply");
    assert!(
        !runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local"
            )
            .unwrap()
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        3
    );
    assert!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 8)
            .unwrap()
            .is_empty()
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 4);
    drop(runtime);
    runtime = load(directory.path());
    for _ in 0..2 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        assert!(
            !runtime
                .legacy_recovery_apply_step(
                    "cluster-a",
                    &source,
                    &state,
                    12,
                    &["local".into()],
                    "local"
                )
                .unwrap()
        );
    }
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let before = serde_json::to_value(&runtime.snapshot).unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    let second_id = journal_row(&tombstones[1]).id;
    connection
        .execute_batch(&format!(
            "CREATE TRIGGER fail_mixed_ack BEFORE DELETE ON source_delivery_journal \
         WHEN OLD.id='{second_id}' BEGIN SELECT RAISE(ABORT,'injected failure'); END;"
        ))
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local"
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(&runtime.snapshot).unwrap(), before);
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 4);
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        5
    );
    connection
        .execute_batch("DROP TRIGGER fail_mixed_ack")
        .unwrap();
    drop(runtime);
    runtime = load(directory.path());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local"
            )
            .unwrap()
    );
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
    assert!(runtime.snapshot.local_history_backfill_completed);
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        7
    );
    let records = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_key, b"3");
}

#[test]
fn mixed_stream_page_tombstone_fork_is_not_acknowledged() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let ordinary = prepare(&mut runtime, 2);
    let first = tombstone(0, b"0", None);
    let good = tombstone(1, b"1", Some(first.segment_hash().unwrap()));
    let tail = tombstone(2, b"2", Some(good.segment_hash().unwrap()));
    let fork = tombstone(1, b"fork", Some(first.segment_hash().unwrap()));
    let fork_row = journal_row(&fork);
    runtime
        .storage
        .append_source_delivery_journal(&[fork_row.clone()])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["tombstone"] = serde_json::json!({
        "next_sequence": 3, "previous_segment_hash": tail.segment_hash().unwrap(), "pending": []
    });
    snapshot["local_history_backfill_inflight"]["segment_ids"] =
        serde_json::json!([journal_row(&ordinary[2]).id, fork_row.id]);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let source = identity(&signing_key());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    // The peer sends an independently valid source chain containing the other branch.
    let state = stage(
        &mut runtime,
        &state,
        &[ordinary[0].clone(), ordinary[1].clone(), first, good, tail],
    );
    assert_eq!(state.phase, "apply");
    for _ in 0..3 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        assert!(
            !runtime
                .legacy_recovery_apply_step(
                    "cluster-a",
                    &source,
                    &state,
                    12,
                    &["local".into()],
                    "local"
                )
                .unwrap()
        );
    }
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let error = runtime
        .legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("not in the verified source chain")
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 2);
    assert!(!runtime.snapshot.local_history_backfill_completed);
}

#[test]
fn live_source_groups_every_deletion_into_the_tombstone_stream() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let key = signing_key();
    let source = identity(&key);
    let segments = runtime
        .queue_local_source_segments(
            "cluster-a",
            source.clone(),
            &key,
            &[
                record(b"ordinary-a", false),
                record(b"deleted-a", true),
                record(b"ordinary-b", false),
                record(b"deleted-b", true),
            ],
            12,
        )
        .unwrap();
    assert_eq!(segments.len(), 2);
    for segment in segments {
        let signed = SignedSegment::from_wire(&segment.wire).unwrap();
        signed.verify_identity(&source).unwrap();
        let canonical = signed.canonical();
        let tombstone_stream = canonical.first_cursor().stream() == "tombstone";
        assert!(
            canonical
                .records()
                .iter()
                .all(|record| record.is_tombstone() == tombstone_stream)
        );
    }
}

#[test]
fn signed_mixed_same_key_wire_applies_deletion_before_ack_and_survives_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let ordinary = prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    let mixed = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 2).unwrap(),
        vec![record(b"same", true), record(b"same", false)],
        Some(ordinary[1].segment_hash().unwrap()),
        11,
        11,
    )
    .unwrap()
    .sign(&signing_key())
    .unwrap();
    let mixed_row = journal_row(&mixed);
    runtime
        .storage
        .append_source_delivery_journal(&[mixed_row.clone()])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = serde_json::json!(4);
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(mixed.segment_hash().unwrap());
    snapshot["local_history_backfill_inflight"]["segment_ids"] = serde_json::json!([mixed_row.id]);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();

    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let state = stage(&mut runtime, &state, &ordinary[..2]);
    assert!(state.mixed_deletions_required);
    assert!(
        !runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local"
            )
            .unwrap()
    );
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let before = serde_json::to_value(&runtime.snapshot).unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_mixed_evidence BEFORE INSERT ON repository_history_records
         WHEN NEW.is_tombstone=1 BEGIN SELECT RAISE(ABORT,'injected failure'); END;",
        )
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local"
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(&runtime.snapshot).unwrap(), before);
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 2);
    connection
        .execute_batch("DROP TRIGGER fail_mixed_evidence")
        .unwrap();
    assert!(
        !runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local"
            )
            .unwrap()
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        0
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 2);
    drop(runtime);
    runtime = load(directory.path());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(state.mixed_deletions_done);
    assert!(
        !runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local"
            )
            .unwrap()
    );
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local"
            )
            .unwrap()
    );
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.record_key != b"same"));
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        3
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    assert!(runtime.snapshot.local_history_backfill_completed);
}

#[test]
fn tombstone_in_ordinary_prefix_is_applied_before_ordinary_page() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    prepare(&mut runtime, 0);
    let key = signing_key();
    let source = identity(&key);
    let first = segment(&key, 0, vec![record(b"deleted", true)], None);
    let second = segment(
        &key,
        1,
        vec![record(b"deleted", false)],
        Some(first.segment_hash().unwrap()),
    );
    let original = segment(
        &key,
        2,
        vec![record(b"kept", false)],
        Some(second.segment_hash().unwrap()),
    );
    let row = journal_row(&original);
    runtime
        .storage
        .append_source_delivery_journal(&[row.clone()])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = serde_json::json!(3);
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(original.segment_hash().unwrap());
    snapshot["local_history_backfill_inflight"]["segment_ids"] = serde_json::json!([row.id]);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let state = stage(&mut runtime, &state, &[first, second]);
    assert!(state.mixed_deletions_required);
    for _ in 0..8 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        if runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local",
            )
            .unwrap()
        {
            break;
        }
    }
    assert!(runtime.snapshot.local_history_backfill_completed);
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].record_key, b"kept");
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        3
    );
}

#[test]
fn mixed_page_rebuilds_changed_deletion_ledger_before_materializing() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let ordinary = prepare(&mut runtime, 2);
    let key = signing_key();
    let source = identity(&key);
    let mixed = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 2).unwrap(),
        vec![record(b"same", true), record(b"same", false)],
        Some(ordinary[1].segment_hash().unwrap()),
        11,
        11,
    )
    .unwrap()
    .sign(&key)
    .unwrap();
    let row = journal_row(&mixed);
    runtime
        .storage
        .append_source_delivery_journal(&[row.clone()])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = serde_json::json!(4);
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(mixed.segment_hash().unwrap());
    snapshot["local_history_backfill_inflight"]["segment_ids"] = serde_json::json!([row.id]);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let state = stage(&mut runtime, &state, &ordinary[..2]);
    assert!(state.mixed_deletions_required);
    for _ in 0..2 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        assert!(
            !runtime
                .legacy_recovery_apply_step(
                    "cluster-a",
                    &source,
                    &state,
                    12,
                    &["local".into()],
                    "local"
                )
                .unwrap()
        );
    }
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(state.mixed_deletions_done);
    runtime.tombstones = TombstoneLedger::new(TOMBSTONE_HORIZON_SECONDS);
    runtime.snapshot.tombstones = runtime.tombstones.checkpoint();
    runtime.persist_control_state().unwrap();
    assert!(
        !runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local"
            )
            .unwrap()
    );
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(!state.mixed_deletions_done);
    assert_eq!(state.mixed_deletions_position, 0);
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        0
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 2);
    for _ in 0..8 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        if runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local",
            )
            .unwrap()
        {
            break;
        }
    }
    assert!(runtime.snapshot.local_history_backfill_completed);
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert!(rows.iter().all(|row| row.record_key != b"same"));
}

#[test]
fn mixed_tombstone_stream_same_key_never_materializes_the_deleted_record() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let ordinary = prepare(&mut runtime, 1);
    let key = signing_key();
    let source = identity(&key);
    let mixed = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "tombstone", 0).unwrap(),
        vec![record(b"same", true), record(b"same", false)],
        None,
        11,
        11,
    )
    .unwrap()
    .sign(&key)
    .unwrap();
    let row = journal_row(&mixed);
    runtime
        .storage
        .append_source_delivery_journal(&[row.clone()])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["tombstone"] = serde_json::json!({
        "next_sequence": 2, "previous_segment_hash": mixed.segment_hash().unwrap(), "pending": []
    });
    snapshot["local_history_backfill_inflight"]["segment_ids"] =
        serde_json::json!([journal_row(&ordinary[1]).id, row.id]);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let state = stage(&mut runtime, &state, &ordinary[..1]);
    assert!(state.mixed_deletions_required);
    for _ in 0..12 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        if runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local",
            )
            .unwrap()
        {
            break;
        }
    }
    assert!(runtime.snapshot.local_history_backfill_completed);
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert!(rows.iter().all(|row| row.record_key != b"same"));
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        3
    );
}

#[test]
fn numbered_tombstone_backpressure_gap_blocks_legacy_ack() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    prepare(&mut runtime, 1);
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["backpressure_gaps"] = serde_json::json!({
        "tombstone\000000000000000000000": {
            "source_epoch": 7, "first_sequence": 0, "last_sequence": 0,
            "start_unix_seconds": 11, "end_unix_seconds": 11
        }
    });
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let error = runtime
        .legacy_original_recovery_state("cluster-a", &identity(&signing_key()), 12)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("tombstone has a backpressure gap")
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}
