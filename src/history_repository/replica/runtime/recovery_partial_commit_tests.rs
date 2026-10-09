use super::*;

fn legacy_partial_commit(path: &std::path::Path) -> RepositoryReplicaRuntime {
    let mut runtime = load(path);
    let key = signing_key();
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    runtime
        .receive_wire(
            "cluster-a",
            &identity(&key),
            &first.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    let receiver = runtime.receiver.as_ref().unwrap().checkpoint().unwrap();
    let handoff = InitialPeerTieredHandoff {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_missing: 1,
        last_missing: 2,
        next_sequence: 3,
        end_unix_seconds: 12,
    };
    runtime.snapshot.initial_peer_backfills.insert(
        "node-b".to_owned(),
        InitialPeerBackfillCheckpoint {
            recovery_generation: 1,
            recovery_handoff: Some(handoff.clone()),
            ..Default::default()
        },
    );
    runtime
        .start_initial_peer_tiered_handoff("node-b", handoff.clone())
        .unwrap();
    runtime
        .update_initial_peer_backfill_checkpoint(
            "node-b",
            None,
            std::collections::BTreeMap::new(),
            true,
            true,
        )
        .unwrap();
    runtime
        .complete_initial_peer_tiered_handoff("node-b", &handoff)
        .unwrap();
    // Recreate the predecessor's partial control commit, including a rotated gap ledger.
    runtime.snapshot.receiver = Some(receiver.clone());
    runtime.receiver =
        Some(SegmentReceiver::from_checkpoint("cluster-a", known_schemas(), receiver).unwrap());
    runtime.snapshot.gaps.clear();
    runtime.snapshot.history_truncated = true;
    runtime.persist_control_state().unwrap();
    drop(runtime);
    let mut runtime = load(path);
    runtime.force_capacity_for_test(0, u64::MAX).unwrap();
    runtime
}

#[test]
fn signed_recovery_repairs_only_the_committed_handoff_then_arms_a_new_generation() {
    let temporary = tempfile::tempdir().unwrap();
    let mut runtime = legacy_partial_commit(temporary.path());
    let before = runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap();
    let preview = runtime.preview_initial_peer_recovery("node-b").unwrap();
    assert_eq!(preview.receiver_watermark, Some(0));
    assert_eq!(
        preview
            .receiver_watermark_repair
            .as_ref()
            .unwrap()
            .last_missing,
        2
    );
    assert_eq!(preview.generation, 2);
    assert_eq!(
        runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        before
    );
    assert!(
        runtime
            .arm_initial_peer_recovery("node-b", "stale")
            .is_err()
    );
    assert_eq!(
        runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        before
    );
    runtime
        .arm_initial_peer_recovery("node-b", &preview.fingerprint)
        .unwrap();
    drop(runtime);
    let runtime = load(temporary.path());
    let checkpoint = runtime.initial_peer_backfill_checkpoint("node-b").unwrap();
    assert_eq!(checkpoint.recovery_generation, 2);
    assert!(!checkpoint.recovery_generation_consumed);
    assert_eq!(checkpoint.recovery_handoff.unwrap().first_missing, 3);
    let cursor = Cursor::new("node-a", 7, "runtime", 3).unwrap();
    assert_eq!(
        runtime
            .receiver
            .as_ref()
            .unwrap()
            .continuous_watermark(&cursor)
            .unwrap()
            .unwrap()
            .sequence(),
        2
    );
    assert!(
        runtime
            .snapshot
            .gaps
            .iter()
            .any(|gap| gap.permanent && gap.first_sequence == 1 && gap.last_sequence == 2)
    );
    assert!(runtime.snapshot.history_truncated);
}

#[test]
fn partial_commit_preview_requires_exact_completed_audit_evidence() {
    for incomplete in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let mut runtime = legacy_partial_commit(temporary.path());
        let checkpoint = runtime
            .snapshot
            .initial_peer_backfills
            .get_mut("node-b")
            .unwrap();
        if incomplete {
            checkpoint.completed = false;
        } else {
            checkpoint.retained_anchor_handoffs.clear();
        }
        assert!(runtime.preview_initial_peer_recovery("node-b").is_err());
    }
}

#[test]
fn partial_commit_recovery_rolls_back_on_write_failure_and_rejects_changed_evidence() {
    let temporary = tempfile::tempdir().unwrap();
    let mut runtime = legacy_partial_commit(temporary.path());
    let preview = runtime.preview_initial_peer_recovery("node-b").unwrap();
    let before = runtime.snapshot.clone();
    runtime.storage.set_query_only_for_test(true).unwrap();
    assert!(
        runtime
            .arm_initial_peer_recovery("node-b", &preview.fingerprint)
            .is_err()
    );
    assert_eq!(
        runtime.initial_peer_backfill_checkpoint("node-b"),
        before.initial_peer_backfills.get("node-b").cloned()
    );
    let cursor = Cursor::new("node-a", 7, "runtime", 3).unwrap();
    assert_eq!(
        runtime
            .receiver
            .as_ref()
            .unwrap()
            .continuous_watermark(&cursor)
            .unwrap()
            .unwrap()
            .sequence(),
        0
    );
    runtime.storage.set_query_only_for_test(false).unwrap();
    drop(runtime);
    let mut runtime = load(temporary.path());
    runtime.force_capacity_for_test(0, u64::MAX).unwrap();
    let preview = runtime.preview_initial_peer_recovery("node-b").unwrap();
    runtime
        .snapshot
        .initial_peer_backfills
        .get_mut("node-b")
        .unwrap()
        .summary_pending_next_cursor = Some("changed".to_owned());
    assert!(
        runtime
            .arm_initial_peer_recovery("node-b", &preview.fingerprint)
            .is_err()
    );
}
