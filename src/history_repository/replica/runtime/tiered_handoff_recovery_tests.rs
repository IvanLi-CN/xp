use std::collections::{BTreeMap, BTreeSet};

use super::{
    InitialPeerRetainedAnchorStream, RetainedAnchorCheckpointUpdate, identity, load, record,
    segment, signing_key,
};

#[test]
fn tiered_handoff_completion_recovers_after_anchor_was_persisted_before_checkpoint_clear() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let anchor = segment(&key, 3, vec![record(b"anchor", false)], None);
    let handoff = super::InitialPeerTieredHandoff {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_missing: 1,
        last_missing: 2,
        next_sequence: 3,
        end_unix_seconds: 12,
    };
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("first wire"),
            11,
        )
        .expect("local progress");
    runtime
        .start_initial_peer_tiered_handoff("node-b", handoff.clone())
        .expect("start tiered handoff");

    // This is the crash window from the old implementation: the anchor and its permanent
    // retention gap are durable, but the active handoff marker is still present.
    runtime
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &anchor.wire_bytes().expect("anchor wire"),
            &[],
            12,
            &["repository-a".to_owned()],
            "repository-a",
            true,
            Some(RetainedAnchorCheckpointUpdate {
                peer_node_id: "node-b".to_owned(),
                response_id: "response-1".to_owned(),
                response_complete: true,
                allowance_complete: true,
                streams: BTreeSet::from([InitialPeerRetainedAnchorStream {
                    source_node_id: "node-a".to_owned(),
                    source_epoch: 7,
                    stream: "runtime".to_owned(),
                }]),
            }),
        )
        .expect("persist retained anchor");
    runtime
        .update_initial_peer_backfill_checkpoint("node-b", None, BTreeMap::new(), true, true)
        .expect("persist completed export");

    runtime
        .complete_initial_peer_tiered_handoff("node-b", &handoff)
        .expect("idempotently complete the already-bridged handoff");
    let checkpoint = runtime
        .initial_peer_backfill_checkpoint("node-b")
        .expect("handoff checkpoint");
    assert!(checkpoint.summary_tiered_handoff.is_none());
    assert!(checkpoint.retained_anchor_handoffs.contains(&handoff));
}

#[test]
fn history_recovery_fingerprint_is_signed_once_and_retries_fail_closed() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let handoff = super::InitialPeerTieredHandoff {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_missing: 1,
        last_missing: 2,
        next_sequence: 3,
        end_unix_seconds: 12,
    };
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("first wire"),
            11,
        )
        .expect("local progress");
    runtime
        .start_initial_peer_tiered_handoff("node-b", handoff.clone())
        .expect("start tiered handoff");
    runtime
        .import_tiered_backfill_records(
            (1..=2)
                .map(|sequence| super::RepositoryTieredBackfillRecord {
                    observed_at_unix_seconds: 12,
                    source_node_id: "node-a".to_owned(),
                    source_epoch: 7,
                    stream: "runtime".to_owned(),
                    sequence,
                    subject_node_id: "subject-a".to_owned(),
                    observer_node_id: "node-a".to_owned(),
                    schema_id: "runtime.v1".to_owned(),
                    schema_version: 1,
                    record_key: format!("tiered-{sequence}").into_bytes(),
                    payload: b"sample".to_vec(),
                    tombstone: false,
                })
                .collect(),
            12,
            &["repository-a".to_owned()],
            "repository-a",
        )
        .expect("import tiered records");
    runtime
        .update_initial_peer_backfill_checkpoint("node-b", None, BTreeMap::new(), true, true)
        .expect("finish tiered export");
    runtime
        .complete_initial_peer_tiered_handoff("node-b", &handoff)
        .expect("bridge tiered gap");

    let quota = runtime
        .runtime_capacity()
        .expect("read capacity")
        .quota_bytes();
    runtime
        .force_capacity_for_test(quota - 1, 512 * 1024 * 1024)
        .expect("set quota budget guard");
    let capacity_error = runtime
        .preview_initial_peer_recovery("node-b")
        .expect_err("recovery budget must fail closed");
    assert!(capacity_error.to_string().contains("quota_shortfall_bytes"));
    runtime
        .force_capacity_for_test(0, 1)
        .expect("set recovery capacity guard");
    assert!(runtime.preview_initial_peer_recovery("node-b").is_err());
    runtime
        .force_capacity_for_test(0, u64::MAX)
        .expect("restore recovery capacity");
    let preview = runtime
        .preview_initial_peer_recovery("node-b")
        .expect("preview recovery");
    assert_eq!(preview.generation, 1);
    assert_eq!(preview.receiver_watermark, Some(2));
    let checkpoint = runtime
        .snapshot
        .initial_peer_backfills
        .get_mut("node-b")
        .expect("recovery checkpoint");
    let original = checkpoint
        .retained_anchor_handoffs
        .iter()
        .next()
        .cloned()
        .expect("previous handoff");
    checkpoint.retained_anchor_handoffs.remove(&original);
    checkpoint
        .retained_anchor_handoffs
        .insert(super::InitialPeerTieredHandoff {
            end_unix_seconds: original.end_unix_seconds + 1,
            ..original.clone()
        });
    assert!(
        runtime
            .arm_initial_peer_recovery("node-b", &preview.fingerprint)
            .is_err()
    );
    let checkpoint = runtime
        .snapshot
        .initial_peer_backfills
        .get_mut("node-b")
        .expect("recovery checkpoint");
    checkpoint.retained_anchor_handoffs.clear();
    checkpoint.retained_anchor_handoffs.insert(original);
    runtime
        .arm_initial_peer_recovery("node-b", &preview.fingerprint)
        .expect("arm recovery");
    assert!(
        runtime
            .arm_initial_peer_recovery("node-b", &preview.fingerprint)
            .is_err()
    );
}

#[test]
fn tiered_handoff_start_rolls_back_memory_when_control_persist_fails() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = super::RepositoryReplicaRuntime::load(storage.clone()).expect("runtime");
    storage
        .set_query_only_for_test(true)
        .expect("enable SQLite write failure");
    let handoff = super::InitialPeerTieredHandoff {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_missing: 1,
        last_missing: 2,
        next_sequence: 3,
        end_unix_seconds: 12,
    };

    assert!(
        runtime
            .start_initial_peer_tiered_handoff("node-b", handoff)
            .is_err()
    );
    assert!(runtime.initial_peer_backfill_checkpoint("node-b").is_none());
}

#[test]
fn peer_backfill_restart_rolls_back_memory_when_control_persist_fails() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = super::RepositoryReplicaRuntime::load(storage.clone()).expect("runtime");
    runtime
        .update_initial_peer_summary_checkpoint_with_retained_anchors(
            "node-b",
            Some("page-2".to_owned()),
            vec!["segment-1".to_owned()],
            Some("page-3".to_owned()),
            false,
            false,
            true,
            BTreeSet::new(),
        )
        .expect("persist checkpoint");
    storage
        .set_query_only_for_test(true)
        .expect("enable SQLite write failure");

    assert!(runtime.restart_initial_peer_backfill("node-b").is_err());
    let checkpoint = runtime
        .initial_peer_backfill_checkpoint("node-b")
        .expect("checkpoint remains in memory");
    assert_eq!(checkpoint.summary_cursor.as_deref(), Some("page-2"));
    assert_eq!(checkpoint.summary_pending_segment_ids, ["segment-1"]);
}
