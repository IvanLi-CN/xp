use super::*;
#[path = "legacy_receiver_watermark_tests.rs"]
mod receiver_watermark;
use crate::state::history_repository::replica::runtime::tests::{
    identity, load, record, segment, signing_key,
};

fn prepare(runtime: &mut RepositoryReplicaRuntime, count: usize) -> Vec<SignedSegment> {
    let key = signing_key();
    let mut hash = None;
    let mut chain = Vec::new();
    for index in 0..=count {
        let signed = segment(
            &key,
            index as u64,
            vec![record(index.to_string().as_bytes(), false)],
            hash,
        );
        hash = Some(signed.segment_hash().unwrap());
        chain.push(signed);
    }
    let original = chain.last().unwrap();
    let wire = original.wire_bytes().unwrap();
    let id = hex::encode(Sha256::digest(&wire));
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: id.clone(),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: identity(&key),
            wire,
        }])
        .unwrap();
    // Build the exact predecessor control shape through serde, preserving private owner fields.
    let mut value = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    value["local_source"]["node_id"] = serde_json::json!("node-a");
    value["local_source"]["source_epoch"] = serde_json::json!(7);
    value["local_source"]["epoch"] = serde_json::json!(7);
    value["local_source"]["streams"] = serde_json::json!({"runtime":{
        "next_sequence":count+1,"previous_segment_hash":hash,"pending":[]
    }});
    value["local_history_backfill_inflight"] = serde_json::json!({
        "segment_ids":[id],"page_cursor":null,"completed":true
    });
    runtime.snapshot = serde_json::from_value(value).unwrap();
    runtime.persist_control_state().unwrap();
    chain
}

fn response(chain: &[SignedSegment]) -> RepositoryRepairBatch {
    RepositoryRepairBatch {
        segments: chain
            .iter()
            .map(|signed| RepositoryReplicaSegment {
                identity: identity(&signing_key()),
                wire: signed.wire_bytes().unwrap(),
            })
            .collect(),
        unavailable_segment_ids: Vec::new(),
        gaps: Vec::new(),
        history_truncated: false,
        retained_stream_boundaries: Vec::new(),
        response_id: None,
        response_content_digest: None,
    }
}

#[test]
fn legacy_recovery_6786_segments_are_staged_incrementally_and_survive_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 6786);
    let source = identity(&signing_key());
    let mut state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    for page in chain[..6786].chunks(64) {
        let previous = state.clone();
        state.pending = page
            .iter()
            .map(|signed| hex::encode(Sha256::digest(signed.wire_bytes().unwrap())))
            .collect();
        runtime
            .legacy_recovery_update_scan(&previous, &state)
            .unwrap();
        runtime
            .legacy_recovery_stage_response("cluster-a", &source, &state, &response(page))
            .unwrap();
        assert_eq!(
            runtime.storage.repository_history_record_count().unwrap(),
            0
        );
        drop(runtime);
        runtime = load(directory.path());
        state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
    }
    assert_eq!(state.phase, "apply");
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    let mut complete = false;
    while !complete {
        state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        complete = runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local",
            )
            .unwrap();
        drop(runtime);
        runtime = load(directory.path());
    }
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
    assert!(runtime.snapshot.local_history_backfill_completed);
    assert!(runtime.snapshot.local_history_backfill_inflight.is_none());
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        6787
    );
    assert!(runtime.snapshot.gaps.is_empty());
}

fn stage_all(
    runtime: &mut RepositoryReplicaRuntime,
    chain: &[SignedSegment],
) -> LegacyRecoveryState {
    let source = identity(&signing_key());
    let mut state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    for page in chain[..chain.len() - 1].chunks(64) {
        let previous = state.clone();
        state.pending = page
            .iter()
            .map(|signed| hex::encode(Sha256::digest(signed.wire_bytes().unwrap())))
            .collect();
        runtime
            .legacy_recovery_update_scan(&previous, &state)
            .unwrap();
        state = runtime
            .legacy_recovery_stage_response("cluster-a", &source, &state, &response(page))
            .unwrap();
    }
    state
}

#[test]
fn legacy_recovery_final_ack_record_and_checkpoint_roll_back_together() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    let mut state = stage_all(&mut runtime, &chain);
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
    state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    let before = serde_json::to_value(&runtime.snapshot).unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_legacy_ack BEFORE DELETE ON source_delivery_journal
        BEGIN SELECT RAISE(ABORT,'injected legacy ACK failure'); END;",
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
        runtime.storage.repository_history_record_count().unwrap(),
        2
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        2
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    connection
        .execute_batch("DROP TRIGGER fail_legacy_ack")
        .unwrap();
    drop(runtime);
    let mut runtime = load(directory.path());
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
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        3
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        3
    );
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
    assert!(runtime.snapshot.local_history_backfill_inflight.is_none());
    assert!(runtime.snapshot.local_history_backfill_completed);
    assert_eq!(
        runtime
            .storage
            .legacy_recovery_staged_after(0)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn legacy_recovery_real_wires_repair_and_future_independent_stream_coexist() {
    use crate::history_sync::{CanonicalSegment, Cursor};
    let directory = tempfile::tempdir().unwrap();
    let consumer_dir = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let source = identity(&signing_key());
    let chain = prepare(&mut runtime, 3);
    let mut state = stage_all(&mut runtime, &chain);
    while !runtime
        .legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
        .unwrap()
    {
        state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
    }
    let ids = chain
        .iter()
        .map(|signed| hex::encode(Sha256::digest(signed.wire_bytes().unwrap())))
        .collect::<Vec<_>>();
    let batch = runtime.repair_batch(&ids).unwrap();
    assert_eq!(batch.segments.len(), 4);
    let batch: RepositoryRepairBatch =
        serde_json::from_slice(&serde_json::to_vec(&batch).unwrap()).unwrap();
    let mut consumer = load(consumer_dir.path());
    for _ in 0..2 {
        consumer
            .receive_wire_batch_from_repository_with_gaps(
                "cluster-a",
                &batch.segments,
                &[],
                12,
                &["local".into()],
                "local",
            )
            .unwrap();
        assert_eq!(
            consumer.storage.repository_history_record_count().unwrap(),
            4
        );
        assert_eq!(
            consumer.storage.repository_history_segment_count().unwrap(),
            4
        );
        drop(consumer);
        consumer = load(consumer_dir.path());
    }
    let derived = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 8, "runtime-backfill-node-a", 0).unwrap(),
        chain[0].canonical().records().to_vec(),
        None,
        10,
        11,
    )
    .unwrap()
    .sign(&signing_key())
    .unwrap();
    for replica in [&mut runtime, &mut consumer] {
        for _ in 0..2 {
            replica
                .receive_wire("cluster-a", &source, &derived.wire_bytes().unwrap(), 12)
                .unwrap();
            assert_eq!(
                replica.storage.repository_history_record_count().unwrap(),
                5
            );
        }
        assert!(replica.snapshot.gaps.is_empty());
    }
}

#[test]
fn legacy_recovery_preserves_other_source_tombstones_without_resetting_receiver() {
    use crate::history_sync::{CanonicalSegment, Cursor};
    use crate::state::history_repository::replica::runtime::tests::identity_for;
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let tombstone = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-b", 7, "tombstone", 0).unwrap(),
        vec![record(b"unrelated-deleted", true)],
        None,
        10,
        11,
    )
    .unwrap()
    .sign(&signing_key())
    .unwrap();
    runtime
        .receive_wire(
            "cluster-a",
            &identity_for(&signing_key(), "node-b"),
            &tombstone.wire_bytes().unwrap(),
            12,
        )
        .unwrap();
    let before = serde_json::to_value(runtime.tombstones.checkpoint()).unwrap();
    let chain = prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    let mut state = stage_all(&mut runtime, &chain);
    while !runtime
        .legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
        .unwrap()
    {
        state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
    }
    assert_eq!(
        serde_json::to_value(runtime.tombstones.checkpoint()).unwrap(),
        before
    );
    assert!(
        runtime
            .receiver
            .as_ref()
            .unwrap()
            .continuous_watermark(&Cursor::new("node-b", 7, "tombstone", 0).unwrap())
            .unwrap()
            .is_some()
    );
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        4
    );
}

#[test]
fn legacy_recovery_rejects_unproven_prefix_and_does_not_ack_or_set_gaps() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let source = identity(&signing_key());
    let initial = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &initial,
                12,
                &["local".into()],
                "local"
            )
            .is_err()
    );
    let mut next = initial.clone();
    let fake = segment(
        &signing_key(),
        0,
        vec![record(b"different-signed-genesis", false)],
        None,
    );
    next.pending = vec![hex::encode(Sha256::digest(fake.wire_bytes().unwrap()))];
    runtime
        .legacy_recovery_update_scan(&initial, &next)
        .unwrap();
    let mut next = runtime
        .legacy_recovery_stage_response("cluster-a", &source, &next, &response(&[fake]))
        .unwrap();
    let previous = next.clone();
    next.pending = vec![hex::encode(Sha256::digest(chain[1].wire_bytes().unwrap()))];
    runtime
        .legacy_recovery_update_scan(&previous, &next)
        .unwrap();
    assert!(
        runtime
            .legacy_recovery_stage_response("cluster-a", &source, &next, &response(&chain[1..2]))
            .is_err()
    );
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        0
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        0
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    assert!(runtime.receiver.is_none());
    assert!(runtime.snapshot.gaps.is_empty());
}

#[test]
fn legacy_recovery_accepts_missing_source_node_id_only_with_exact_signed_identity_and_epoch() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]
        .as_object_mut()
        .unwrap()
        .remove("node_id");
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    drop(runtime);
    let mut runtime = load(directory.path());
    let different = ed25519_dalek::SigningKey::from_bytes(&[91; 32]);
    assert!(
        runtime
            .legacy_original_recovery_state("cluster-a", &identity(&different), 12)
            .is_err()
    );
    assert!(
        runtime
            .legacy_original_recovery_state("cluster-a", &identity(&signing_key()), 12)
            .is_ok()
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    let source = identity(&signing_key());
    let mut state = stage_all(&mut runtime, &chain);
    while !runtime
        .legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
        .unwrap()
    {
        state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
    }
    drop(runtime);
    let runtime = load(directory.path());
    assert_eq!(runtime.snapshot.local_source.node_id(), Some("node-a"));
    assert_eq!(runtime.snapshot.local_source.epoch(), 7);
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn legacy_recovery_two_pages_advance_cursor_once_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_history_backfill_inflight"]["page_cursor"] = serde_json::json!("cursor-1");
    snapshot["local_history_backfill_inflight"]["completed"] = serde_json::json!(false);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let source = identity(&signing_key());
    let mut state = stage_all(&mut runtime, &chain);
    while !runtime
        .legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
        .unwrap()
    {
        drop(runtime);
        runtime = load(directory.path());
        state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
    }
    drop(runtime);
    let mut runtime = load(directory.path());
    assert_eq!(runtime.local_history_backfill_cursor(), Some("cursor-1"));
    assert!(!runtime.snapshot.local_history_backfill_completed);
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        3
    );
    let original = segment(
        &signing_key(),
        3,
        vec![record(b"second-page", false)],
        Some(chain[2].segment_hash().unwrap()),
    );
    let wire = original.wire_bytes().unwrap();
    let id = hex::encode(Sha256::digest(&wire));
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: id.clone(),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        }])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = serde_json::json!(4);
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(original.segment_hash().unwrap());
    snapshot["local_history_backfill_inflight"] =
        serde_json::json!({"segment_ids":[id],"page_cursor":"cursor-2","completed":true});
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    drop(runtime);
    let mut runtime = load(directory.path());
    let mut state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    assert_eq!(state.phase, "scan");
    let previous = state.clone();
    state.pending = chain
        .iter()
        .map(|segment| hex::encode(Sha256::digest(segment.wire_bytes().unwrap())))
        .collect();
    runtime
        .legacy_recovery_update_scan(&previous, &state)
        .unwrap();
    state = runtime
        .legacy_recovery_stage_response("cluster-a", &source, &state, &response(&chain))
        .unwrap();
    while !runtime
        .legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
        .unwrap()
    {
        drop(runtime);
        runtime = load(directory.path());
        state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
    }
    drop(runtime);
    let runtime = load(directory.path());
    assert_eq!(runtime.local_history_backfill_cursor(), Some("cursor-2"));
    assert!(runtime.snapshot.local_history_backfill_completed);
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        4
    );
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        4
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
fn legacy_recovery_never_clears_a_permanent_prefix_gap() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    prepare(&mut runtime, 2);
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["gaps"] = serde_json::json!([{"source_node_id":"node-a","source_epoch":7,
        "stream":"runtime","first_sequence":0,"last_sequence":1,
        "start_unix_seconds":0,"end_unix_seconds":1,"permanent":true}]);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    let before = serde_json::to_value(&runtime.snapshot).unwrap();
    assert!(
        runtime
            .legacy_original_recovery_state("cluster-a", &identity(&signing_key()), 12)
            .is_err()
    );
    assert_eq!(serde_json::to_value(&runtime.snapshot).unwrap(), before);
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}

#[test]
fn legacy_recovery_rejects_replayed_completed_page_cursor_without_ack() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 0);
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_history_backfill_inflight"]["page_cursor"] = serde_json::json!("cursor-1");
    snapshot["local_history_backfill_inflight"]["completed"] = serde_json::json!(false);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    let source = identity(&signing_key());
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
    let second = segment(
        &signing_key(),
        1,
        vec![record(b"next", false)],
        Some(chain[0].segment_hash().unwrap()),
    );
    let wire = second.wire_bytes().unwrap();
    let id = hex::encode(Sha256::digest(&wire));
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: id.clone(),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: source.clone(),
            wire,
        }])
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["runtime"]["next_sequence"] = serde_json::json!(2);
    snapshot["local_source"]["streams"]["runtime"]["previous_segment_hash"] =
        serde_json::json!(second.segment_hash().unwrap());
    snapshot["local_history_backfill_inflight"] =
        serde_json::json!({"segment_ids":[id], "page_cursor":"cursor-1", "completed":false});
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    let before = serde_json::to_value(&runtime.snapshot).unwrap();
    assert!(
        runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .is_err()
    );
    assert_eq!(serde_json::to_value(&runtime.snapshot).unwrap(), before);
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        1
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}

#[test]
fn legacy_recovery_predecessor_page_without_creation_time_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["last_verified_unix_seconds"] = serde_json::json!(12);
    snapshot["local_history_backfill_inflight"]
        .as_object_mut()
        .unwrap()
        .remove("created_at_unix_seconds");
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();
    let source = identity(&signing_key());
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
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        3
    );
    assert!(
        runtime
            .storage
            .source_delivery_journal()
            .unwrap()
            .is_empty()
    );
    assert!(runtime.snapshot.gaps.is_empty());
}

#[test]
fn legacy_recovery_missing_source_node_requires_matching_identity_and_epoch() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    prepare(&mut runtime, 2);
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]
        .as_object_mut()
        .unwrap()
        .remove("node_id");
    runtime.snapshot = serde_json::from_value(snapshot.clone()).unwrap();
    let wrong_key = ed25519_dalek::SigningKey::from_bytes(&[91; 32]);
    assert!(
        runtime
            .legacy_original_recovery_state("cluster-a", &identity(&wrong_key), 12)
            .is_err()
    );
    snapshot["local_source"]["epoch"] = serde_json::json!(8);
    runtime.snapshot = serde_json::from_value(snapshot.clone()).unwrap();
    assert!(
        runtime
            .legacy_original_recovery_state("cluster-a", &identity(&signing_key()), 12)
            .is_err()
    );
    snapshot["local_source"]["epoch"] = serde_json::json!(7);
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &identity(&signing_key()), 12)
        .unwrap();
    assert_eq!(state.epoch, 7);
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}

#[test]
fn legacy_recovery_source_tombstones_report_missing_prefix_proof_without_ack() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    prepare(&mut runtime, 2);
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["streams"]["tombstone"] = serde_json::json!({
        "next_sequence": 1, "previous_segment_hash": vec![42; 32], "pending": []
    });
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    let state = runtime
        .legacy_original_recovery_state("cluster-a", &identity(&signing_key()), 12)
        .unwrap();
    assert_eq!(state.targets["tombstone"].head, 1);
    assert_eq!(state.phase, "scan");
    assert!(
        runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &identity(&signing_key()),
                &state,
                12,
                &["local".into()],
                "local"
            )
            .is_err()
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}

#[path = "legacy_tombstone_recovery_tests.rs"]
mod legacy_tombstone_recovery_tests;

#[path = "legacy_mixed_page_tests.rs"]
mod legacy_mixed_page_tests;

#[path = "legacy_field_shape_tests.rs"]
mod legacy_field_shape_tests;

#[path = "legacy_suffix_tests.rs"]
mod legacy_suffix_tests;

#[test]
fn legacy_recovery_suppresses_known_foreign_deletion_and_acks_original_wire() {
    use crate::state::history_repository::replica::runtime::tests::identity_for;

    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let other_key = ed25519_dalek::SigningKey::from_bytes(&[13; 32]);
    let deletion = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-b", 8, "tombstone", 0).unwrap(),
        vec![record(b"0", true)],
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
            &deletion.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    let source = identity(&signing_key());
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
    drop(runtime);
    let mut runtime = load(directory.path());
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
    let rows = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.record_key != b"0"));
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        4
    );
}

#[test]
fn legacy_recovery_rejects_unknown_wire_below_ahead_receiver_watermark() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let chain = prepare(&mut runtime, 2);
    let key = signing_key();
    let source = identity(&key);
    let mut hash = None;
    for index in 0..5 {
        let conflicting = segment(
            &key,
            index,
            vec![record(format!("other-{index}").as_bytes(), false)],
            hash,
        );
        hash = Some(conflicting.segment_hash().unwrap());
        runtime
            .receive_wire("cluster-a", &source, &conflicting.wire_bytes().unwrap(), 11)
            .unwrap();
    }
    let state = stage_all(&mut runtime, &chain);
    let before = serde_json::to_value(&runtime.snapshot).unwrap();
    let original = runtime.storage.source_delivery_journal().unwrap();
    let error = runtime
        .legacy_recovery_apply_step("cluster-a", &source, &state, 12, &["local".into()], "local")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unknown wire below the receiver watermark")
    );
    assert_eq!(serde_json::to_value(&runtime.snapshot).unwrap(), before);
    assert_eq!(
        runtime.storage.source_delivery_journal().unwrap()[0].wire,
        original[0].wire
    );
}
