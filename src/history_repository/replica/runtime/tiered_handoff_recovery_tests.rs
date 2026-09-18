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
