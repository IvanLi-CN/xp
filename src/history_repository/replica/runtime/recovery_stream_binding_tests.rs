use std::collections::BTreeMap;

use super::{
    InitialPeerBackfillCheckpoint, InitialPeerTieredHandoff, identity, load, record, segment,
    signing_key,
};

#[test]
fn recovery_stream_binding_survives_an_independent_handoff_and_restart() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let cursor = first.canonical().first_cursor();
    let peer = xp_test_fixtures::secondary_node_id();
    let handoff = InitialPeerTieredHandoff {
        source_node_id: cursor.source_node_id().to_owned(),
        source_epoch: cursor.source_epoch(),
        stream: cursor.stream().to_owned(),
        first_missing: 1,
        last_missing: 2,
        next_sequence: 3,
        end_unix_seconds: 12,
    };
    let recovery = InitialPeerTieredHandoff {
        stream: "connections".to_owned(),
        ..handoff.clone()
    };
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("initial watermark");
    runtime.snapshot.initial_peer_backfills.insert(
        peer.to_owned(),
        InitialPeerBackfillCheckpoint {
            recovery_generation: 1,
            recovery_generation_consumed: true,
            recovery_handoff: Some(recovery.clone()),
            ..Default::default()
        },
    );
    runtime
        .start_initial_peer_tiered_handoff(peer, handoff.clone())
        .expect("independent stream handoff");
    runtime
        .update_initial_peer_backfill_checkpoint(peer, None, BTreeMap::new(), true, true)
        .expect("complete export");
    runtime
        .complete_initial_peer_tiered_handoff(peer, &handoff)
        .expect("complete independent stream");
    drop(runtime);
    let runtime = load(temporary.path());
    assert_eq!(
        runtime
            .receiver
            .as_ref()
            .expect("receiver survives restart")
            .continuous_watermark(cursor)
            .expect("watermark")
            .expect("stream progress")
            .sequence(),
        handoff.last_missing,
        "handoff completion must commit its receiver watermark with the checkpoint"
    );
    let checkpoint = runtime
        .initial_peer_backfill_checkpoint(peer)
        .expect("checkpoint");
    assert_eq!(checkpoint.recovery_generation, 1);
    assert!(checkpoint.recovery_generation_consumed);
    assert_eq!(checkpoint.recovery_handoff, Some(recovery));
    assert!(checkpoint.retained_anchor_handoffs.contains(&handoff));
    assert!(checkpoint.summary_tiered_handoff.is_none());
}

#[test]
fn recovery_stream_binding_rejects_unrelated_handoff_before_consumption() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let peer = xp_test_fixtures::secondary_node_id();
    let handoff = InitialPeerTieredHandoff {
        source_node_id: xp_test_fixtures::primary_node_id().to_owned(),
        source_epoch: 7,
        stream: "connections".to_owned(),
        first_missing: 19793,
        last_missing: 50227,
        next_sequence: 50228,
        end_unix_seconds: 12,
    };
    let mut runtime = load(temporary.path());
    runtime.snapshot.initial_peer_backfills.insert(
        peer.to_owned(),
        InitialPeerBackfillCheckpoint {
            recovery_generation: 1,
            recovery_handoff: Some(handoff.clone()),
            ..Default::default()
        },
    );
    let before = runtime
        .initial_peer_backfill_checkpoint(peer)
        .expect("checkpoint");
    let unrelated = InitialPeerTieredHandoff {
        stream: "tombstone".to_owned(),
        ..handoff
    };
    assert!(
        runtime
            .start_initial_peer_tiered_handoff(peer, unrelated)
            .is_err()
    );
    assert_eq!(runtime.initial_peer_backfill_checkpoint(peer), Some(before));
}
