use super::*;

fn source_tombstone(sequence: u64, key: &[u8], previous: Option<[u8; 32]>) -> SignedSegment {
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

fn set_tombstone_tail(runtime: &mut RepositoryReplicaRuntime, end: u64, hash: [u8; 32]) {
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["tombstone"] = serde_json::json!({
        "next_sequence":end, "previous_segment_hash":hash, "pending":[]
    });
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
}

fn stage_page(
    runtime: &mut RepositoryReplicaRuntime,
    state: &LegacyRecoveryState,
    wires: &[SignedSegment],
) -> LegacyRecoveryState {
    let mut request = state.clone();
    request.pending = wires
        .iter()
        .map(|wire| hex::encode(Sha256::digest(wire.wire_bytes().unwrap())))
        .collect();
    runtime
        .legacy_recovery_update_scan(state, &request)
        .unwrap();
    runtime
        .legacy_recovery_stage_response(
            "cluster-a",
            &identity(&signing_key()),
            &request,
            &response(wires),
        )
        .unwrap()
}

#[test]
fn legacy_tombstone_tail_is_applied_before_originals_and_survives_restart_and_new_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let first = source_tombstone(0, b"0", None);
    let second = source_tombstone(1, b"2", Some(first.segment_hash().unwrap()));
    set_tombstone_tail(&mut runtime, 1, first.segment_hash().unwrap());
    let source = identity(&signing_key());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    // Deliberately stage ordinary wires before the deletion wire.
    let state = stage_page(&mut runtime, &state, &chain[..2]);
    let state = stage_page(&mut runtime, &state, std::slice::from_ref(&first));
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
        1
    );
    drop(runtime);
    runtime = load(directory.path());
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
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(state.tombstones_applied);
    set_tombstone_tail(&mut runtime, 2, second.segment_hash().unwrap());
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
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.targets["tombstone"].next, 1);
    let state = stage_page(&mut runtime, &state, std::slice::from_ref(&second));
    assert_eq!(state.phase, "apply");
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
    assert_eq!(rows[0].record_key, b"1");
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        5
    );
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn legacy_tombstone_tail_hash_mismatch_does_not_materialize_or_ack() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let tombstone = source_tombstone(0, b"0", None);
    set_tombstone_tail(&mut runtime, 1, [42; 32]);
    let source = identity(&signing_key());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let mut state = stage_page(&mut runtime, &state, &chain[..2]);
    let before = state.clone();
    state.pending = vec![hex::encode(Sha256::digest(tombstone.wire_bytes().unwrap()))];
    runtime
        .legacy_recovery_update_scan(&before, &state)
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_stage_response("cluster-a", &source, &state, &response(&[tombstone]))
            .is_err()
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        0
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}

#[test]
fn legacy_tombstones_use_local_journal_in_bounded_steps() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let ordinary = prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    let mut previous = None;
    let mut rows = Vec::new();
    for sequence in 0..70 {
        let signed = source_tombstone(sequence, sequence.to_string().as_bytes(), previous);
        previous = Some(signed.segment_hash().unwrap());
        let wire = signed.wire_bytes().unwrap();
        rows.push(SourceDeliveryJournalRow {
            id: hex::encode(Sha256::digest(&wire)),
            stream: "tombstone".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        });
    }
    runtime
        .storage
        .append_source_delivery_journal(&rows)
        .unwrap();
    set_tombstone_tail(&mut runtime, 70, previous.unwrap());
    for expected in 1..=70 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        assert_eq!(state.targets["tombstone"].next, expected);
        if expected == 35 {
            drop(runtime);
            runtime = load(directory.path());
        }
    }
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let state = stage_page(&mut runtime, &state, &ordinary[..2]);
    assert_eq!(state.phase, "apply");
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
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        73
    );
    let remaining = runtime.storage.source_delivery_journal().unwrap();
    assert_eq!(remaining.len(), 70);
    assert!(remaining.iter().all(|row| row.stream == "tombstone"));
}

#[test]
fn legacy_tombstone_fixed_tail_does_not_move_during_scan() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let ordinary = prepare(&mut runtime, 2);
    let first = source_tombstone(0, b"0", None);
    let second = source_tombstone(1, b"2", Some(first.segment_hash().unwrap()));
    set_tombstone_tail(&mut runtime, 1, first.segment_hash().unwrap());
    let source = identity(&signing_key());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.targets["tombstone"].head, 1);
    set_tombstone_tail(&mut runtime, 2, second.segment_hash().unwrap());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.targets["tombstone"].head, 1);
    let state = stage_page(&mut runtime, &state, &ordinary[..2]);
    let state = stage_page(&mut runtime, &state, &[first]);
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
    assert_eq!(state.targets["tombstone"].head, 2);
    assert_eq!(state.targets["tombstone"].next, 1);
    assert_eq!(state.phase, "scan");
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}

#[test]
fn legacy_tombstone_apply_failure_rolls_back_receiver_and_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let ordinary = prepare(&mut runtime, 2);
    let tombstone = source_tombstone(0, b"0", None);
    set_tombstone_tail(&mut runtime, 1, tombstone.segment_hash().unwrap());
    let source = identity(&signing_key());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let state = stage_page(&mut runtime, &state, &ordinary[..2]);
    let state = stage_page(&mut runtime, &state, &[tombstone]);
    let before = serde_json::to_value(&runtime.snapshot).unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_legacy_tombstone BEFORE INSERT ON repository_history_segments
             BEGIN SELECT RAISE(ABORT,'injected failure'); END;",
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
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        0
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    connection
        .execute_batch("DROP TRIGGER fail_legacy_tombstone")
        .unwrap();
    drop(runtime);
    let mut runtime = load(directory.path());
    let restored = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(restored.tombstone_apply_position, 0);
    assert!(
        !runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &restored,
                12,
                &["local".into()],
                "local"
            )
            .unwrap()
    );
}

#[test]
fn legacy_tombstone_unverified_receiver_never_acks() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let ordinary = prepare(&mut runtime, 2);
    let first = source_tombstone(0, b"0", None);
    let second = source_tombstone(1, b"2", Some(first.segment_hash().unwrap()));
    set_tombstone_tail(&mut runtime, 2, second.segment_hash().unwrap());
    let source = identity(&signing_key());
    runtime
        .receive_initial_backfill_wire_from_repository(
            "cluster-a",
            &source,
            &second.wire_bytes().unwrap(),
            11,
            &["local".into()],
            "local",
        )
        .unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let state = stage_page(&mut runtime, &state, &ordinary[..2]);
    let state = stage_page(&mut runtime, &state, &[first, second]);
    let error = runtime
        .legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
        .unwrap_err();
    assert!(error.to_string().contains("unverified"));
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    assert!(!runtime.snapshot.local_history_backfill_completed);
}

#[test]
fn mixed_tombstone_stream_deletes_existing_same_source_record_before_page_ack() {
    use crate::state::history_repository::replica::runtime::tests::identity_for;

    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let ordinary = prepare(&mut runtime, 1);
    let key = signing_key();
    let source = identity(&key);
    let earlier = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "tombstone", 0).unwrap(),
        vec![record(b"same", false), record(b"unrelated", false)],
        None,
        11,
        11,
    )
    .unwrap()
    .sign(&key)
    .unwrap();
    runtime
        .receive_wire("cluster-a", &source, &earlier.wire_bytes().unwrap(), 11)
        .unwrap();

    // Exact same-stream coverage is scoped to the original source and epoch.
    let other_key = ed25519_dalek::SigningKey::from_bytes(&[13; 32]);
    let other = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-b", 7, "tombstone", 0).unwrap(),
        vec![record(b"same", false)],
        None,
        11,
        11,
    )
    .unwrap()
    .sign(&other_key)
    .unwrap();
    runtime
        .receive_wire(
            "cluster-a",
            &identity_for(&other_key, "node-b"),
            &other.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    let mixed = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "tombstone", 2).unwrap(),
        vec![record(b"same", true), record(b"same", false)],
        Some(earlier.segment_hash().unwrap()),
        11,
        11,
    )
    .unwrap()
    .sign(&key)
    .unwrap();
    let wire = mixed.wire_bytes().unwrap();
    let mixed_id = hex::encode(Sha256::digest(&wire));
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: mixed_id.clone(),
            stream: "tombstone".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        }])
        .unwrap();
    set_tombstone_tail(&mut runtime, 4, mixed.segment_hash().unwrap());
    let ordinary_id = hex::encode(Sha256::digest(ordinary[1].wire_bytes().unwrap()));
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_history_backfill_inflight"]["segment_ids"] =
        serde_json::json!([ordinary_id, mixed_id]);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();

    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let state = stage_page(&mut runtime, &state, &[earlier, ordinary[0].clone()]);
    assert_eq!(state.targets["tombstone"].next, 2);
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
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

    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 16)
        .unwrap();
    assert!(
        !rows
            .iter()
            .any(|row| row.source_node_id == "node-a" && row.record_key == b"same")
    );
    assert!(
        rows.iter()
            .any(|row| row.source_node_id == "node-a" && row.record_key == b"unrelated")
    );
    assert!(
        rows.iter()
            .any(|row| row.source_node_id == "node-b" && row.record_key == b"same")
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 2);
    let cursor = Cursor::new("node-a", 7, "tombstone", 0).unwrap();
    assert_eq!(
        runtime
            .receiver
            .as_ref()
            .unwrap()
            .continuous_watermark(&cursor)
            .unwrap()
            .unwrap()
            .sequence(),
        1
    );
    drop(runtime);
    runtime = load(directory.path());
    for _ in 0..16 {
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
        .repository_history_records(None, None, None, 0, 16)
        .unwrap();
    assert!(
        !rows
            .iter()
            .any(|row| row.source_node_id == "node-a" && row.record_key == b"same")
    );
    assert!(
        rows.iter()
            .any(|row| row.source_node_id == "node-b" && row.record_key == b"same")
    );
    assert!(
        rows.iter()
            .any(|row| row.source_node_id == "node-a" && row.record_key == b"unrelated")
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        5
    );
}
