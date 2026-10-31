use super::legacy_mixed_page_tests::{journal_row, stage};
use super::*;

#[test]
fn legacy_recovery_rejects_pending_page_forked_from_advanced_live_tail() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    // The durable source advanced, but its signed successor belongs to a different chain.
    let successor = segment(
        &signing_key(),
        3,
        vec![record(b"forked-successor", false)],
        Some([77; 32]),
    );
    let wire = successor.wire_bytes().unwrap();
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: hex::encode(Sha256::digest(&wire)),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        }])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 4.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(successor.segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let before = runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap();
    let recovery_before = runtime.storage.legacy_recovery_checkpoint().unwrap();
    let journal_before = runtime.storage.source_delivery_journal().unwrap().len();

    let result = runtime.legacy_recovery_verify_suffix_step("cluster-a", &source, &state);
    assert!(
        result.is_err(),
        "a signed fork cannot prove the live source chain"
    );
    assert_eq!(
        runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        before
    );
    assert_eq!(
        runtime.storage.legacy_recovery_checkpoint().unwrap(),
        recovery_before
    );
    assert_eq!(
        runtime.storage.source_delivery_journal().unwrap().len(),
        journal_before
    );
}

#[test]
fn legacy_recovery_verifies_advanced_source_in_bounded_pages_across_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    let mut hash = chain.last().unwrap().segment_hash().unwrap();
    let mut rows = Vec::new();
    for sequence in 3..263 {
        let signed = segment(
            &signing_key(),
            sequence,
            vec![record(sequence.to_string().as_bytes(), false)],
            Some(hash),
        );
        hash = signed.segment_hash().unwrap();
        let wire = signed.wire_bytes().unwrap();
        rows.push(SourceDeliveryJournalRow {
            id: hex::encode(Sha256::digest(&wire)),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        });
    }
    for page in rows.chunks(64) {
        runtime
            .storage
            .append_source_delivery_journal(page)
            .unwrap();
    }
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 263.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(hash);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        !runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap()
    );
    // New live capture must not move the proof's captured durable anchor indefinitely.
    let later = segment(
        &signing_key(),
        263,
        vec![record(b"later-live-capture", false)],
        Some(hash),
    );
    let wire = later.wire_bytes().unwrap();
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: hex::encode(Sha256::digest(&wire)),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        }])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 264.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(later.segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    drop(runtime);

    let mut runtime = load(directory.path());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.targets["runtime"].suffix_next, 67);
    assert_eq!(state.targets["runtime"].source_end, 263);
    assert_eq!(state.suffix_staged_count, 64);
    assert_eq!(
        runtime
            .storage
            .legacy_recovery_suffix_after(0)
            .unwrap()
            .len(),
        64
    );
    let mut previous = 67;
    loop {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        let complete = runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap();
        drop(runtime);
        runtime = load(directory.path());
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        assert!(state.targets["runtime"].suffix_next - previous <= 64);
        previous = state.targets["runtime"].suffix_next;
        assert_eq!(state.targets["runtime"].source_end, 263);
        if complete {
            break;
        }
    }
    drop(runtime);

    let mut runtime = load(directory.path());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.targets["runtime"].suffix_next, 263);
    assert_eq!(state.suffix_staged_count, 260);
    assert_eq!(
        runtime.storage.source_delivery_journal().unwrap().len(),
        262
    );
    assert!(runtime.snapshot.local_history_backfill_inflight.is_some());
}

#[test]
fn legacy_recovery_fetches_acknowledged_successor_from_peer_without_acknowledging_page() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    let successor = segment(
        &signing_key(),
        3,
        vec![record(b"acknowledged-successor", false)],
        Some(chain.last().unwrap().segment_hash().unwrap()),
    );
    // The peer may also return newer captures beyond the anchored source checkpoint.
    let later = segment(
        &signing_key(),
        4,
        vec![record(b"later-live-capture", false)],
        Some(successor.segment_hash().unwrap()),
    );
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 4.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(successor.segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        !runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap()
    );
    drop(runtime);
    let mut runtime = load(directory.path());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(state.suffix_from_peer);
    let mut requested = state.clone();
    requested.pending = [&successor, &later]
        .into_iter()
        .map(|segment| hex::encode(Sha256::digest(segment.wire_bytes().unwrap())))
        .collect();
    runtime
        .legacy_recovery_update_scan(&state, &requested)
        .unwrap();
    let result = runtime
        .legacy_recovery_stage_response(
            "cluster-a",
            &source,
            &requested,
            &response(&[successor, later]),
        )
        .unwrap();
    assert!(!result.suffix_from_peer);
    assert_eq!(result.targets["runtime"].suffix_next, 4);
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    assert!(runtime.snapshot.local_history_backfill_inflight.is_some());
}

#[test]
fn legacy_recovery_combines_peer_and_local_successor_wires() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    let peer_only = segment(
        &signing_key(),
        3,
        vec![record(b"peer-only", false)],
        Some(chain.last().unwrap().segment_hash().unwrap()),
    );
    let local_only = segment(
        &signing_key(),
        4,
        vec![record(b"local-only", false)],
        Some(peer_only.segment_hash().unwrap()),
    );
    let wire = local_only.wire_bytes().unwrap();
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: hex::encode(Sha256::digest(&wire)),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        }])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 5.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(local_only.segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        !runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap()
    );
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(state.suffix_from_peer);
    let mut requested = state.clone();
    requested.pending = vec![hex::encode(Sha256::digest(peer_only.wire_bytes().unwrap()))];
    runtime
        .legacy_recovery_update_scan(&state, &requested)
        .unwrap();
    let result = runtime
        .legacy_recovery_stage_response("cluster-a", &source, &requested, &response(&[peer_only]))
        .unwrap();
    assert!(
        !result.suffix_from_peer,
        "resume the local journal after peer progress"
    );
    assert_eq!(result.targets["runtime"].suffix_next, 4);
    assert!(
        runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &result)
            .unwrap()
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 2);
}

#[test]
fn peer_suffix_tombstone_is_durable_and_applied_after_original_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    let successor = segment(
        &signing_key(),
        3,
        vec![record(b"0", true)],
        Some(chain.last().unwrap().segment_hash().unwrap()),
    );
    let expected_wire = successor.wire_bytes().unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 4.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(successor.segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        !runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap()
    );
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let mut requested = state.clone();
    requested.pending = vec![hex::encode(Sha256::digest(&expected_wire))];
    runtime
        .legacy_recovery_update_scan(&state, &requested)
        .unwrap();
    runtime
        .legacy_recovery_stage_response("cluster-a", &source, &requested, &response(&[successor]))
        .unwrap();
    drop(runtime);
    runtime = load(directory.path());
    assert_eq!(
        runtime.storage.legacy_recovery_suffix_after(0).unwrap()[0]
            .1
            .wire,
        expected_wire
    );
    let state = stage_all(&mut runtime, &chain);
    assert!(state.mixed_deletions_required);
    let mut saw_page_receipt = false;
    for _ in 0..32 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        if state.page_receipted {
            saw_page_receipt = true;
            assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
        }
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
    assert!(saw_page_receipt);
    assert!(runtime.snapshot.local_history_backfill_completed);
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        4
    );
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.record_key != b"0"));
}

#[test]
fn suffix_staging_failure_rolls_back_verified_cursor_and_keeps_journal() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 1);
    let source = identity(&signing_key());
    let suffix = segment(
        &signing_key(),
        2,
        vec![record(b"0", true)],
        Some(chain.last().unwrap().segment_hash().unwrap()),
    );
    let wire = suffix.wire_bytes().unwrap();
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: hex::encode(Sha256::digest(&wire)),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        }])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 3.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(suffix.segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let before = runtime.storage.legacy_recovery_checkpoint().unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection.execute_batch(
        "CREATE TRIGGER fail_suffix_stage BEFORE INSERT ON repository_legacy_recovery_suffix_wires
         BEGIN SELECT RAISE(ABORT,'injected failure'); END;"
    ).unwrap();
    assert!(
        runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .is_err()
    );
    assert_eq!(
        runtime.storage.legacy_recovery_checkpoint().unwrap(),
        before
    );
    assert!(
        runtime
            .storage
            .legacy_recovery_suffix_after(0)
            .unwrap()
            .is_empty()
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 2);
    connection
        .execute_batch("DROP TRIGGER fail_suffix_stage")
        .unwrap();
    drop(runtime);
    runtime = load(directory.path());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap()
    );
    assert_eq!(
        runtime
            .storage
            .legacy_recovery_suffix_after(0)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn missing_verified_suffix_evidence_never_acknowledges_the_page() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 1);
    let source = identity(&signing_key());
    let suffix = segment(
        &signing_key(),
        2,
        vec![record(b"0", true)],
        Some(chain.last().unwrap().segment_hash().unwrap()),
    );
    let wire = suffix.wire_bytes().unwrap();
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: hex::encode(Sha256::digest(&wire)),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        }])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 3.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(suffix.segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap()
    );
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute("DELETE FROM repository_legacy_recovery_suffix_wires", [])
        .unwrap();
    let state = stage_all(&mut runtime, &chain);
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
    let error = runtime
        .legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("suffix deletion evidence is incomplete")
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 2);
    assert!(!runtime.snapshot.local_history_backfill_completed);
    assert!(runtime.snapshot.gaps.is_empty());
}

#[test]
fn mixed_page_captures_suffix_deletion_and_retries_transaction_failure() {
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
    let suffix = segment(
        &key,
        4,
        vec![record(b"0", true)],
        Some(mixed.segment_hash().unwrap()),
    );
    let row = journal_row(&mixed);
    runtime
        .storage
        .append_source_delivery_journal(&[row.clone(), journal_row(&suffix)])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = serde_json::json!(5);
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(suffix.segment_hash().unwrap());
    snapshot["local_history_backfill_inflight"]["segment_ids"] = serde_json::json!([row.id]);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap()
    );
    drop(runtime);
    runtime = load(directory.path());
    assert_eq!(
        runtime
            .storage
            .legacy_recovery_suffix_after(0)
            .unwrap()
            .len(),
        1
    );
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let state = stage(&mut runtime, &state, &ordinary[..2]);
    assert!(state.mixed_deletions_required);
    for _ in 0..16 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        if state.page_receipted {
            break;
        }
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
    assert!(state.page_receipted);
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 3);
    let later = segment(
        &key,
        5,
        vec![record(b"later", false)],
        Some(suffix.segment_hash().unwrap()),
    );
    runtime
        .storage
        .append_source_delivery_journal(&[journal_row(&later)])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 6.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(later.segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    drop(runtime);
    runtime = load(directory.path());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.targets["runtime"].source_end, 5);
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    let suffix_id = journal_row(&suffix).id;
    connection
        .execute_batch(&format!(
            "CREATE TRIGGER fail_legacy_suffix BEFORE INSERT ON repository_history_segments
         WHEN NEW.id='{suffix_id}' BEGIN SELECT RAISE(ABORT,'suffix failure'); END;"
        ))
        .unwrap();
    let mut failed = false;
    for _ in 0..16 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        let before = serde_json::to_value(&runtime.snapshot).unwrap();
        match runtime.legacy_recovery_apply_step(
            "cluster-a",
            &source,
            &state,
            12,
            &["local".into()],
            "local",
        ) {
            Err(_) => {
                assert_eq!(serde_json::to_value(&runtime.snapshot).unwrap(), before);
                assert_eq!(state.suffix_apply_position, 0);
                failed = true;
                break;
            }
            Ok(false) => {}
            Ok(true) => panic!("suffix failure must precede ACK"),
        }
    }
    assert!(failed);
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 4);
    connection
        .execute_batch("DROP TRIGGER fail_legacy_suffix")
        .unwrap();
    drop(runtime);
    runtime = load(directory.path());
    for _ in 0..32 {
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
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 3);
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        4
    );
    let source_state = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    assert_eq!(
        source_state["local_source"]["streams"]["runtime"]["next_sequence"],
        6
    );
    assert_eq!(
        source_state["local_source"]["streams"]["runtime"]["previous_segment_hash"],
        serde_json::json!(later.segment_hash().unwrap())
    );
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].record_key, b"1");
}

#[test]
fn mixed_page_rechecks_source_tail_advanced_during_recovery() {
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
    let suffix = segment(
        &key,
        4,
        vec![record(b"later", false)],
        Some(mixed.segment_hash().unwrap()),
    );
    runtime
        .storage
        .append_source_delivery_journal(&[journal_row(&suffix)])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = serde_json::json!(5);
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(suffix.segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
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
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap()
    );
    for _ in 0..20 {
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
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 2);
}
