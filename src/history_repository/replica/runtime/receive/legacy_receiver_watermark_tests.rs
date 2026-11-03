use super::*;

#[path = "legacy_duplicate_retention_tests.rs"]
mod duplicate_retention;

fn ahead_receiver(
    directory: &std::path::Path,
    receiver_last: usize,
    fork: bool,
    wide: bool,
) -> (RepositoryReplicaRuntime, Vec<SignedSegment>) {
    let mut runtime = load(directory);
    let mut chain = prepare(&mut runtime, 96);
    let key = signing_key();
    let source = identity(&key);
    for sequence in 97..240 {
        chain.push(segment(
            &key,
            sequence,
            vec![record(sequence.to_string().as_bytes(), false)],
            Some(chain.last().unwrap().segment_hash().unwrap()),
        ));
    }
    let receiver_chain = chain.clone();
    if fork || wide {
        chain.truncate(198);
        let first = 198;
        let records = if wide { 3 } else { 1 };
        chain.push(segment(
            &key,
            first,
            (0..records)
                .map(|i| record(format!("branch-{}", first + i).as_bytes(), false))
                .collect(),
            Some(chain.last().unwrap().segment_hash().unwrap()),
        ));
        for sequence in first + records..240 {
            chain.push(segment(
                &key,
                sequence,
                vec![record(sequence.to_string().as_bytes(), false)],
                Some(chain.last().unwrap().segment_hash().unwrap()),
            ));
        }
    }
    for signed in &chain[97..] {
        let wire = signed.wire_bytes().unwrap();
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
    }
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = 240.into();
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(chain.last().unwrap().segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    for signed in &receiver_chain[..=receiver_last] {
        runtime
            .receive_wire("cluster-a", &source, &signed.wire_bytes().unwrap(), 11)
            .unwrap();
    }
    // Retention can evict wire IDs while the receiver's true continuous watermark survives.
    let database = rusqlite::Connection::open(directory.join("history.sqlite3")).unwrap();
    database
        .execute("DELETE FROM repository_history_segments", [])
        .unwrap();
    drop(database);
    (runtime, chain)
}

#[test]
fn evicted_page_recovers_against_the_actual_receiver_tip_with_bounded_restartable_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let (mut runtime, chain) = ahead_receiver(directory.path(), 239, false, false);
    let source = identity(&signing_key());
    let before_receiver =
        serde_json::to_value(runtime.receiver.as_ref().unwrap().checkpoint().unwrap()).unwrap();
    let before_source =
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap()["local_source"].clone();
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        240
    );
    let mut state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let mut suffix_pages = 0;
    while state.targets["runtime"].suffix_next < state.targets["runtime"].source_end {
        runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap();
        suffix_pages += 1;
        drop(runtime);
        runtime = load(directory.path());
        state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
    }
    assert!(suffix_pages >= 3);
    stage_all(&mut runtime, &chain[..97]);
    let mut completed = false;
    for _ in 0..40 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        completed = runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local",
            )
            .unwrap();
        assert!(
            runtime
                .storage
                .legacy_recovery_checkpoint()
                .unwrap()
                .unwrap()
                .len()
                <= 128 * 1024
        );
        drop(runtime);
        runtime = load(directory.path());
        if completed {
            break;
        }
    }
    assert!(
        completed,
        "valid original wire must converge despite the evicted ID window"
    );
    assert!(runtime.snapshot.local_history_backfill_completed);
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        240
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        0
    );
    assert_eq!(
        serde_json::to_value(runtime.receiver.as_ref().unwrap().checkpoint().unwrap()).unwrap(),
        before_receiver
    );
    assert_eq!(
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap()["local_source"],
        before_source
    );
    let journal = runtime.storage.source_delivery_journal().unwrap();
    assert_eq!(journal.len(), 143);
    assert!(
        journal
            .iter()
            .all(|row| row.wire != chain[96].wire_bytes().unwrap())
    );
    assert!(runtime.snapshot.gaps.is_empty());
}

fn stage_evidence(runtime: &mut RepositoryReplicaRuntime, chain: &[SignedSegment]) {
    let source = identity(&signing_key());
    loop {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        if state.targets["runtime"].suffix_next == state.targets["runtime"].source_end {
            break;
        }
        runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap();
    }
    stage_all(runtime, &chain[..97]);
}

fn apply(runtime: &mut RepositoryReplicaRuntime) -> Result<bool, RepositoryRuntimeError> {
    let source = identity(&signing_key());
    let state = runtime.legacy_original_recovery_state("cluster-a", &source, 12)?;
    runtime.legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
}

#[test]
fn intermediate_receiver_tip_is_proven_before_continuing_to_the_later_source_tail() {
    let directory = tempfile::tempdir().unwrap();
    let (mut runtime, chain) = ahead_receiver(directory.path(), 199, false, false);
    stage_evidence(&mut runtime, &chain);
    let before_source =
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap()["local_source"].clone();
    let mut completed = false;
    for _ in 0..48 {
        completed = apply(&mut runtime).unwrap();
        drop(runtime);
        runtime = load(directory.path());
        if completed {
            break;
        }
    }
    assert!(completed);
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        240
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        40
    );
    let (tip, hash) = runtime
        .receiver
        .as_ref()
        .unwrap()
        .continuous_watermark_with_hash(chain[0].canonical().first_cursor())
        .unwrap()
        .unwrap();
    assert_eq!(tip.sequence(), 239);
    assert_eq!(hash, chain.last().unwrap().segment_hash().unwrap());
    assert_eq!(
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap()["local_source"],
        before_source
    );
    assert_eq!(
        runtime.storage.source_delivery_journal().unwrap().len(),
        143
    );
    assert!(runtime.snapshot.gaps.is_empty());
}

#[test]
fn valid_source_tail_does_not_authorize_a_fork_or_a_receiver_tip_inside_a_segment() {
    for wide in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (mut runtime, chain) = ahead_receiver(directory.path(), 199, true, wide);
        stage_evidence(&mut runtime, &chain);
        assert_rejected_without_ack(directory.path(), runtime, "tip boundary or hash mismatch");
    }
}

fn assert_rejected_without_ack(
    directory: &std::path::Path,
    mut runtime: RepositoryReplicaRuntime,
    message: &str,
) {
    let before_gaps = serde_json::to_value(&runtime.snapshot.gaps).unwrap();
    let before_control = runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap();
    let before_journal =
        serde_json::to_value(runtime.storage.source_delivery_journal().unwrap()).unwrap();
    let before_receiver =
        serde_json::to_value(runtime.receiver.as_ref().unwrap().checkpoint().unwrap()).unwrap();
    let before_records = runtime.storage.repository_history_record_count().unwrap();
    let mut error = None;
    for _ in 0..24 {
        let checkpoint = runtime.storage.legacy_recovery_checkpoint().unwrap();
        match apply(&mut runtime) {
            Err(failure) => {
                assert_eq!(
                    runtime.storage.legacy_recovery_checkpoint().unwrap(),
                    checkpoint
                );
                error = Some(failure.to_string());
                break;
            }
            Ok(complete) => assert!(!complete, "invalid evidence must never ACK"),
        }
        drop(runtime);
        runtime = load(directory);
    }
    assert!(
        error.as_ref().is_some_and(|error| error.contains(message)),
        "{error:?}"
    );
    assert_eq!(
        runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        before_control
    );
    assert_eq!(
        serde_json::to_value(runtime.storage.source_delivery_journal().unwrap()).unwrap(),
        before_journal
    );
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        before_records
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        0
    );
    drop(runtime);
    let runtime = load(directory);
    assert_eq!(
        serde_json::to_value(runtime.receiver.as_ref().unwrap().checkpoint().unwrap()).unwrap(),
        before_receiver
    );
    assert!(runtime.snapshot.local_history_backfill_inflight.is_some());
    assert_eq!(
        serde_json::to_value(&runtime.snapshot.gaps).unwrap(),
        before_gaps
    );
}

#[test]
fn evicted_receiver_proof_rejects_missing_prefix_or_suffix_after_restart() {
    for suffix in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (mut runtime, chain) = ahead_receiver(directory.path(), 199, false, false);
        stage_evidence(&mut runtime, &chain);
        let database =
            rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
        let table = if suffix {
            "repository_legacy_recovery_suffix_wires"
        } else {
            "repository_legacy_recovery_wires"
        };
        assert_eq!(
            database
                .execute(&format!("DELETE FROM {table} WHERE position=44"), [])
                .unwrap(),
            1
        );
        drop(database);
        drop(runtime);
        assert_rejected_without_ack(
            directory.path(),
            load(directory.path()),
            "missing or conflicting link",
        );
    }
}

#[test]
fn evicted_receiver_proof_authenticates_each_staged_wire_after_restart() {
    for fault in ["signature", "identity", "chain"] {
        let directory = tempfile::tempdir().unwrap();
        let (mut runtime, chain) = ahead_receiver(directory.path(), 199, false, false);
        stage_evidence(&mut runtime, &chain);
        let database =
            rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
        if fault == "identity" {
            let other = identity(&ed25519_dalek::SigningKey::from_bytes(&[19; 32]));
            database
                .execute(
                    "UPDATE repository_legacy_recovery_wires SET identity=?1 WHERE position=44",
                    [serde_json::to_vec(&other).unwrap()],
                )
                .unwrap();
        } else {
            let mut wire = chain[43].wire_bytes().unwrap();
            if fault == "signature" {
                *wire.last_mut().unwrap() ^= 1;
            } else {
                wire = segment(
                    &signing_key(),
                    43,
                    vec![record(b"43", false)],
                    Some([55; 32]),
                )
                .wire_bytes()
                .unwrap();
            }
            let id = hex::encode(Sha256::digest(&wire));
            database
                .execute(
                    "UPDATE repository_legacy_recovery_wires SET wire=?1,id=?2 WHERE position=44",
                    rusqlite::params![wire, id],
                )
                .unwrap();
        }
        drop(database);
        drop(runtime);
        assert_rejected_without_ack(directory.path(), load(directory.path()), "");
    }
}

#[test]
fn receiver_progress_during_proof_requires_a_new_tip_and_survives_restart() {
    let directory = tempfile::tempdir().unwrap();
    let (mut runtime, chain) = ahead_receiver(directory.path(), 199, false, false);
    stage_evidence(&mut runtime, &chain);
    for _ in 0..4 {
        assert!(!apply(&mut runtime).unwrap());
    }
    let source = identity(&signing_key());
    for signed in &chain[200..210] {
        runtime
            .receive_wire("cluster-a", &source, &signed.wire_bytes().unwrap(), 12)
            .unwrap();
    }
    drop(runtime);
    let mut runtime = load(directory.path());
    let mut completed = false;
    for _ in 0..48 {
        completed = apply(&mut runtime).unwrap();
        if completed {
            break;
        }
    }
    assert!(completed);
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        240
    );
    assert_eq!(
        runtime.storage.source_delivery_journal().unwrap().len(),
        143
    );
    let (tip, hash) = runtime
        .receiver
        .as_ref()
        .unwrap()
        .continuous_watermark_with_hash(chain[0].canonical().first_cursor())
        .unwrap()
        .unwrap();
    assert_eq!(tip.sequence(), 239);
    assert_eq!(hash, chain.last().unwrap().segment_hash().unwrap());
    assert!(runtime.snapshot.gaps.is_empty());
}

#[test]
fn evicted_receiver_ack_failure_rolls_back_receipt_and_control() {
    let directory = tempfile::tempdir().unwrap();
    let (mut runtime, chain) = ahead_receiver(directory.path(), 199, false, false);
    stage_evidence(&mut runtime, &chain);
    let database = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    database
        .execute_batch(
            "CREATE TRIGGER fail_verified_ack BEFORE DELETE ON source_delivery_journal
        BEGIN SELECT RAISE(FAIL, 'verified ack rollback'); END;",
        )
        .unwrap();
    drop(database);
    let original = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap()
        ["local_history_backfill_inflight"]["segment_ids"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let mut failed = false;
    for _ in 0..40 {
        let control = runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap();
        let checkpoint = runtime.storage.legacy_recovery_checkpoint().unwrap();
        let journal =
            serde_json::to_value(runtime.storage.source_delivery_journal().unwrap()).unwrap();
        match apply(&mut runtime) {
            Err(error) => {
                assert!(error.to_string().contains("verified ack rollback"));
                assert_eq!(
                    runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
                    control
                );
                assert_eq!(
                    runtime.storage.legacy_recovery_checkpoint().unwrap(),
                    checkpoint
                );
                assert_eq!(
                    serde_json::to_value(runtime.storage.source_delivery_journal().unwrap())
                        .unwrap(),
                    journal
                );
                assert!(
                    runtime
                        .storage
                        .source_delivery_journal()
                        .unwrap()
                        .iter()
                        .any(|row| { row.id == original })
                );
                failed = true;
                break;
            }
            Ok(complete) => assert!(!complete),
        }
        drop(runtime);
        runtime = load(directory.path());
    }
    assert!(failed, "ACK failure must leave the original page pending");
}

#[test]
fn network_scan_cannot_supply_or_replace_receiver_proof() {
    let directory = tempfile::tempdir().unwrap();
    let (mut runtime, chain) = ahead_receiver(directory.path(), 199, false, false);
    stage_evidence(&mut runtime, &chain);
    let source = identity(&signing_key());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let before = runtime.storage.legacy_recovery_checkpoint().unwrap();
    let mut value = serde_json::to_value(&state).unwrap();
    value["receiver_proof"] = serde_json::to_value(suffix::ReceiverProof::default()).unwrap();
    value["receiver_proof"]["phase"] = 3.into();
    let forged = serde_json::from_value(value).unwrap();
    assert!(
        runtime
            .legacy_recovery_update_scan(&state, &forged)
            .is_err()
    );
    assert_eq!(
        runtime.storage.legacy_recovery_checkpoint().unwrap(),
        before
    );
    assert!(runtime.snapshot.local_history_backfill_inflight.is_some());
    assert_eq!(
        runtime.storage.source_delivery_journal().unwrap().len(),
        144
    );
}

#[test]
fn predecessor_checkpoint_without_receiver_proof_resumes_the_exact_cas_encoding() {
    let directory = tempfile::tempdir().unwrap();
    let (mut runtime, chain) = ahead_receiver(directory.path(), 199, false, false);
    stage_evidence(&mut runtime, &chain);
    let bytes = runtime
        .storage
        .legacy_recovery_checkpoint()
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        value.get("receiver_proof").is_none(),
        "default must preserve predecessor bytes"
    );
    drop(runtime);
    let mut runtime = load(directory.path());
    assert!(!apply(&mut runtime).unwrap());
    let checkpoint = runtime
        .storage
        .legacy_recovery_checkpoint()
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&checkpoint).unwrap();
    assert!(value.get("receiver_proof").is_some());
    drop(runtime);
    let mut runtime = load(directory.path());
    assert!(!apply(&mut runtime).unwrap());
    assert_eq!(
        runtime.storage.source_delivery_journal().unwrap().len(),
        144
    );
}

#[test]
fn evicted_receiver_proof_preserves_a_permanent_gap_in_the_later_suffix() {
    let directory = tempfile::tempdir().unwrap();
    let (mut runtime, chain) = ahead_receiver(directory.path(), 199, false, false);
    stage_evidence(&mut runtime, &chain);
    let mut value = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    value["gaps"] = serde_json::json!([{"source_node_id":"node-a","source_epoch":7,
        "stream":"runtime","first_sequence":150,"last_sequence":151,
        "start_unix_seconds":0,"end_unix_seconds":1,"permanent":true}]);
    runtime.snapshot = serde_json::from_value(value).unwrap();
    runtime.persist_control_state().unwrap();
    assert_rejected_without_ack(directory.path(), runtime, "permanent gap");
}

#[test]
fn receiver_beyond_captured_tail_extends_only_the_verified_suffix_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let (mut runtime, mut chain) = ahead_receiver(directory.path(), 199, false, false);
    stage_evidence(&mut runtime, &chain);
    assert!(!apply(&mut runtime).unwrap());
    let source = identity(&signing_key());
    for sequence in 240..260 {
        chain.push(segment(
            &signing_key(),
            sequence,
            vec![record(sequence.to_string().as_bytes(), false)],
            Some(chain.last().unwrap().segment_hash().unwrap()),
        ));
    }
    let rows = chain[240..]
        .iter()
        .map(|signed| {
            let wire = signed.wire_bytes().unwrap();
            SourceDeliveryJournalRow {
                id: hex::encode(Sha256::digest(&wire)),
                stream: "runtime".into(),
                closed_at_unix_seconds: 11,
                identity: source.clone(),
                wire,
            }
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .append_source_delivery_journal(&rows)
        .unwrap();
    let mut value = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    value["local_source"]["streams"]["runtime"]["next_sequence"] = 260.into();
    value["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(chain.last().unwrap().segment_hash().unwrap());
    runtime.snapshot = serde_json::from_value(value).unwrap();
    for signed in &chain[200..] {
        runtime
            .receive_wire("cluster-a", &source, &signed.wire_bytes().unwrap(), 12)
            .unwrap();
    }
    runtime.persist_control_state().unwrap();
    drop(runtime);
    let mut runtime = load(directory.path());
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.targets["runtime"].source_end, 260);
    assert_eq!(state.targets["runtime"].suffix_next, 240);
    assert!(state.receiver_proof.is_empty());
    assert!(
        runtime
            .legacy_recovery_verify_suffix_step("cluster-a", &source, &state)
            .unwrap()
    );
    drop(runtime);
    let mut runtime = load(directory.path());
    let mut complete = false;
    for _ in 0..48 {
        complete = apply(&mut runtime).unwrap();
        if complete {
            break;
        }
    }
    assert!(complete);
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        260
    );
    assert_eq!(
        runtime.storage.source_delivery_journal().unwrap().len(),
        163
    );
    let (tip, hash) = runtime
        .receiver
        .as_ref()
        .unwrap()
        .continuous_watermark_with_hash(chain[0].canonical().first_cursor())
        .unwrap()
        .unwrap();
    assert_eq!(tip.sequence(), 259);
    assert_eq!(hash, chain.last().unwrap().segment_hash().unwrap());
    assert!(runtime.snapshot.gaps.is_empty());
}
