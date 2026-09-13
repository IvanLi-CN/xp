use std::collections::{BTreeMap, BTreeSet};

use super::{
    InitialPeerRetainedAnchorStream, RetainedAnchorCheckpointUpdate, identity, load, record,
    segment, signing_key,
};
use crate::state::history_repository::replica::RepositoryRepairBatch;

#[test]
fn initial_backfill_accepts_a_truncated_tail_after_local_progress() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let anchor = segment(&key, 3, vec![record(b"retained", false)], Some([42; 32]));
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("first wire"),
            11,
        )
        .expect("local progress");
    runtime.snapshot.history_truncated = true;

    runtime
        .receive_initial_backfill_wire_from_repository_with_gaps(
            "cluster-a",
            &identity,
            &anchor.wire_bytes().expect("anchor wire"),
            &[],
            12,
            &["repository-a".to_owned()],
            "repository-a",
            true,
        )
        .expect("truncated tail anchor");

    assert_eq!(
        runtime
            .receiver
            .as_ref()
            .expect("receiver")
            .continuous_watermarks()[0]
            .sequence(),
        3
    );
    assert!(runtime.snapshot.gaps.iter().any(|gap| {
        gap.first_sequence == 1
            && gap.last_sequence == 2
            && gap.permanent
            && gap.reason.as_deref() == Some("source_retention_expired")
    }));
}

#[test]
fn relay_batch_preserves_history_truncated_marker() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime.snapshot.history_truncated = true;

    let page = runtime.relay_batch("repository-b").expect("relay page");
    let decoded = RepositoryRepairBatch::from_relay_payload(&page.payload).expect("relay payload");

    assert!(decoded.history_truncated);
}

#[test]
fn history_truncation_status_is_available_to_source_relay() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    assert!(!runtime.history_truncated());

    runtime.snapshot.history_truncated = true;

    assert!(runtime.history_truncated());
}

#[test]
fn ordinary_backfill_rejects_a_truncated_sequence_gap() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let anchor = segment(&key, 3, vec![record(b"retained", false)], Some([42; 32]));
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("local progress");
    runtime.snapshot.history_truncated = true;

    let error = runtime
        .receive_initial_backfill_wire_from_repository_with_gaps(
            "cluster-a",
            &identity,
            &anchor.wire_bytes().expect("wire"),
            &[],
            12,
            &["repository-a".to_owned()],
            "repository-a",
            false,
        )
        .expect_err("ordinary backfill must reject a sequence gap");
    assert!(matches!(error, super::RepositoryRuntimeError::Protocol(_)));
}

#[test]
fn tiered_handoff_bridges_a_retained_sequence_gap_before_repair_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let next = segment(&key, 3, vec![record(b"next", false)], Some([42; 32]));
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("first wire"),
            11,
        )
        .expect("local progress");
    let handoff = super::InitialPeerTieredHandoff {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_missing: 1,
        last_missing: 2,
        next_sequence: 3,
        end_unix_seconds: 12,
    };
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

    runtime
        .receive_initial_backfill_wire_from_repository(
            "cluster-a",
            &identity,
            &next.wire_bytes().expect("next wire"),
            13,
            &["repository-a".to_owned()],
            "repository-a",
        )
        .expect("repair anchor after tiered bridge");
    assert_eq!(
        runtime
            .receiver
            .as_ref()
            .expect("receiver")
            .continuous_watermarks()[0]
            .sequence(),
        3
    );
    assert!(runtime.snapshot.gaps.iter().any(|gap| {
        gap.first_sequence == 1
            && gap.last_sequence == 2
            && gap.permanent
            && gap.reason.as_deref() == Some("source_retention_expired")
    }));
}

#[test]
fn tiered_handoff_detects_a_retained_anchor_without_a_previous_hash() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let anchor = segment(&key, 3, vec![record(b"anchor", false)], None);
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("first wire"),
            11,
        )
        .expect("local progress");

    let handoff = runtime
        .tiered_handoff_for_sequence_gap(&anchor.wire_bytes().expect("anchor wire"))
        .expect("detect tiered handoff")
        .expect("retained anchor crosses the local watermark");

    assert_eq!(handoff.first_missing, 1);
    assert_eq!(handoff.last_missing, 2);
    assert_eq!(handoff.next_sequence, 3);
}

#[test]
fn retained_sequence_gap_is_allowed_once_for_a_later_repair_page() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let anchor = segment(&key, 3, vec![record(b"retained", false)], Some([42; 32]));
    let next = segment(
        &key,
        4,
        vec![record(b"next", false)],
        Some(anchor.segment_hash().expect("anchor hash")),
    );
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("first wire"),
            11,
        )
        .expect("local progress");

    assert!(
        runtime
            .can_accept_retained_sequence_gap(&anchor.wire_bytes().expect("anchor wire"), false)
            .expect("retained anchor probe")
    );
    runtime
        .receive_initial_backfill_wire_from_repository_with_gaps(
            "cluster-a",
            &identity,
            &anchor.wire_bytes().expect("anchor wire"),
            &[],
            12,
            &["repository-a".to_owned()],
            "repository-a",
            true,
        )
        .expect("retained tail anchor");
    assert!(
        !runtime
            .can_accept_retained_sequence_gap(&next.wire_bytes().expect("next wire"), true)
            .expect("second retained anchor probe")
    );
    runtime
        .receive_initial_backfill_wire_from_repository(
            "cluster-a",
            &identity,
            &next.wire_bytes().expect("next wire"),
            13,
            &["repository-a".to_owned()],
            "repository-a",
        )
        .expect("contiguous continuation");
}

#[test]
fn retained_sequence_gap_probe_is_scoped_to_each_source_stream() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first_a = segment(&key, 0, vec![record(b"first-a", false)], None);
    let first_b = segment_at_stream(&key, "traffic", 0, vec![record(b"first-b", false)], None);
    let anchor_a = segment(&key, 3, vec![record(b"retained-a", false)], Some([42; 32]));
    let anchor_b = segment_at_stream(
        &key,
        "traffic",
        3,
        vec![record(b"retained-b", false)],
        Some([43; 32]),
    );
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first_a.wire_bytes().expect("first a wire"),
            11,
        )
        .expect("local stream a progress");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first_b.wire_bytes().expect("first b wire"),
            11,
        )
        .expect("local stream b progress");

    let mut retained_anchor_streams = BTreeSet::new();
    for anchor in [&anchor_a, &anchor_b] {
        let wire = anchor.wire_bytes().expect("anchor wire");
        let decoded = crate::history_sync::SignedSegment::from_wire(&wire).expect("segment");
        let cursor = decoded.canonical().first_cursor();
        let stream_key = (
            cursor.source_node_id().to_owned(),
            cursor.source_epoch(),
            cursor.stream().to_owned(),
        );
        if !retained_anchor_streams.contains(&stream_key)
            && runtime
                .can_accept_retained_sequence_gap(&wire, false)
                .expect("retained anchor probe")
        {
            retained_anchor_streams.insert(stream_key);
        }
    }
    assert_eq!(retained_anchor_streams.len(), 2);
}

#[test]
fn retained_anchor_checkpoint_and_segment_commit_retry_as_one_unit() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let anchor = segment(&key, 3, vec![record(b"anchor", false)], Some([42; 32]));
    let second_gap = segment(&key, 7, vec![record(b"second-gap", false)], Some([43; 32]));
    let update = || RetainedAnchorCheckpointUpdate {
        peer_node_id: "node-b".to_owned(),
        response_id: "response-1".to_owned(),
        response_complete: true,
        allowance_complete: true,
        streams: std::collections::BTreeSet::from([InitialPeerRetainedAnchorStream {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
        }]),
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
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &anchor.wire_bytes().expect("anchor wire"),
            &[],
            12,
            &["repository-a".to_owned()],
            "repository-a",
            true,
            Some(update()),
        )
        .expect("first retained anchor");
    assert!(
        runtime
            .initial_peer_backfill_checkpoint("node-b")
            .expect("checkpoint")
            .retained_anchor_repair_response_seen
    );

    let error = runtime
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &second_gap.wire_bytes().expect("second gap wire"),
            &[],
            13,
            &["repository-a".to_owned()],
            "repository-a",
            false,
            Some(update()),
        )
        .expect_err("same-stream second gap must be rejected");
    assert!(matches!(error, super::RepositoryRuntimeError::Protocol(_)));

    runtime
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &anchor.wire_bytes().expect("anchor retry wire"),
            &[],
            14,
            &["repository-a".to_owned()],
            "repository-a",
            false,
            Some(update()),
        )
        .expect("duplicate anchor retry");
    let error = runtime
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &second_gap.wire_bytes().expect("second gap retry wire"),
            &[],
            15,
            &["repository-a".to_owned()],
            "repository-a",
            false,
            Some(update()),
        )
        .expect_err("retry must not consume a second retained allowance");
    assert!(matches!(error, super::RepositoryRuntimeError::Protocol(_)));
}

#[test]
fn tiered_handoff_preserves_the_bounded_repair_request_for_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    let pending = vec![
        "anchor-segment".to_owned(),
        "unavailable-segment".to_owned(),
    ];
    runtime
        .update_initial_peer_summary_checkpoint_with_retained_anchor_response(
            "node-b",
            Some("summary-page".to_owned()),
            pending.clone(),
            Some("next-summary-page".to_owned()),
            false,
            true,
            Some("bounded-response".to_owned()),
            false,
            false,
            BTreeSet::new(),
        )
        .expect("persist bounded repair request");
    let handoff = super::InitialPeerTieredHandoff {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_missing: 1,
        last_missing: 2,
        next_sequence: 3,
        end_unix_seconds: 12,
    };
    runtime
        .start_initial_peer_tiered_handoff("node-b", handoff)
        .expect("schedule tiered handoff");

    let checkpoint = runtime
        .initial_peer_backfill_checkpoint("node-b")
        .expect("handoff checkpoint");
    assert_eq!(checkpoint.summary_pending_segment_ids, pending);
    assert_eq!(
        checkpoint.retained_anchor_repair_response_id.as_deref(),
        Some("bounded-response")
    );
    assert!(!checkpoint.retained_anchor_repair_response_seen);
}

#[test]
fn retained_anchor_response_identity_survives_partial_failure() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first_runtime = segment(&key, 0, vec![record(b"first-runtime", false)], None);
    let first_traffic = segment_at_stream(
        &key,
        "traffic",
        0,
        vec![record(b"first-traffic", false)],
        None,
    );
    let anchor_runtime = segment(
        &key,
        3,
        vec![record(b"anchor-runtime", false)],
        Some([42; 32]),
    );
    let anchor_traffic = segment_at_stream(
        &key,
        "traffic",
        3,
        vec![record(b"anchor-traffic", false)],
        Some([43; 32]),
    );
    let response_id = RepositoryRepairBatch {
        segments: vec![
            super::RepositoryReplicaSegment {
                identity: identity.clone(),
                wire: anchor_runtime.wire_bytes().expect("runtime repair wire"),
            },
            super::RepositoryReplicaSegment {
                identity: identity.clone(),
                wire: anchor_traffic.wire_bytes().expect("traffic repair wire"),
            },
        ],
        unavailable_segment_ids: Vec::new(),
        gaps: Vec::new(),
        history_truncated: true,
        response_id: None,
    }
    .response_id_digest()
    .expect("repair response identity");
    let update =
        |response_id: &str, response_complete: bool, streams| RetainedAnchorCheckpointUpdate {
            peer_node_id: "node-b".to_owned(),
            response_id: response_id.to_owned(),
            response_complete,
            allowance_complete: response_complete,
            streams,
        };
    let mut runtime = load(temporary.path());
    for first in [&first_runtime, &first_traffic] {
        runtime
            .receive_wire(
                "cluster-a",
                &identity,
                &first.wire_bytes().expect("first wire"),
                11,
            )
            .expect("local progress");
    }

    let runtime_stream = BTreeSet::from([InitialPeerRetainedAnchorStream {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
    }]);
    runtime
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &anchor_runtime.wire_bytes().expect("runtime anchor wire"),
            &[],
            12,
            &["repository-a".to_owned()],
            "repository-a",
            true,
            Some(update(&response_id, false, runtime_stream.clone())),
        )
        .expect("first stream in response");

    let error = runtime
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &anchor_traffic.wire_bytes().expect("traffic anchor wire"),
            &[],
            13,
            &["repository-a".to_owned()],
            "repository-a",
            true,
            Some(update("response-2", false, runtime_stream.clone())),
        )
        .expect_err("a replacement response must be rejected");
    assert!(matches!(error, super::RepositoryRuntimeError::Storage(_)));
    assert_eq!(
        runtime
            .initial_peer_backfill_checkpoint("node-b")
            .expect("in-progress checkpoint")
            .retained_anchor_repair_response_id
            .as_deref(),
        Some(response_id.as_str())
    );

    let mut runtime = load(temporary.path());

    let streams = BTreeSet::from([
        InitialPeerRetainedAnchorStream {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
        },
        InitialPeerRetainedAnchorStream {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "traffic".to_owned(),
        },
    ]);
    runtime
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &anchor_traffic.wire_bytes().expect("traffic retry wire"),
            &[],
            14,
            &["repository-a".to_owned()],
            "repository-a",
            true,
            Some(update(&response_id, true, streams.clone())),
        )
        .expect("same response retry consumes remaining stream");
    let checkpoint = runtime
        .initial_peer_backfill_checkpoint("node-b")
        .expect("completed checkpoint");
    assert!(checkpoint.retained_anchor_repair_response_seen);
    assert!(checkpoint.retained_anchor_repair_response_id.is_none());
    assert_eq!(checkpoint.retained_anchor_streams, streams);

    runtime
        .update_initial_peer_summary_checkpoint_with_retained_anchor_response(
            "node-b",
            None,
            vec!["later-segment".to_owned()],
            None,
            false,
            false,
            Some("later-response".to_owned()),
            true,
            true,
            checkpoint.retained_anchor_streams,
        )
        .expect("a later wire-bounded repair response may use a new identity");
    let checkpoint = runtime
        .initial_peer_backfill_checkpoint("node-b")
        .expect("checkpoint after later response");
    assert!(checkpoint.retained_anchor_repair_response_seen);
    assert!(checkpoint.retained_anchor_repair_response_id.is_none());
    assert_eq!(
        checkpoint.summary_pending_segment_ids,
        vec!["later-segment".to_owned()]
    );
}

#[test]
fn bounded_repair_responses_keep_stream_allowances_until_the_summary_page_drains() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first_runtime = segment(&key, 0, vec![record(b"first-runtime", false)], None);
    let first_traffic = segment_at_stream(
        &key,
        "traffic",
        0,
        vec![record(b"first-traffic", false)],
        None,
    );
    let anchor_runtime = segment(
        &key,
        3,
        vec![record(b"anchor-runtime", false)],
        Some([42; 32]),
    );
    let anchor_traffic = segment_at_stream(
        &key,
        "traffic",
        3,
        vec![record(b"anchor-traffic", false)],
        Some([43; 32]),
    );
    let runtime_stream = InitialPeerRetainedAnchorStream {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
    };
    let traffic_stream = InitialPeerRetainedAnchorStream {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "traffic".to_owned(),
    };
    let mut runtime = load(temporary.path());
    for first in [&first_runtime, &first_traffic] {
        runtime
            .receive_wire(
                "cluster-a",
                &identity,
                &first.wire_bytes().expect("first wire"),
                11,
            )
            .expect("local progress");
    }
    runtime
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &anchor_runtime.wire_bytes().expect("runtime anchor wire"),
            &[],
            12,
            &["repository-a".to_owned()],
            "repository-a",
            true,
            Some(RetainedAnchorCheckpointUpdate {
                peer_node_id: "node-b".to_owned(),
                response_id: "first-bounded-response".to_owned(),
                response_complete: true,
                allowance_complete: false,
                streams: BTreeSet::from([runtime_stream.clone()]),
            }),
        )
        .expect("first bounded response");
    let checkpoint = runtime
        .initial_peer_backfill_checkpoint("node-b")
        .expect("checkpoint after first response");
    assert!(!checkpoint.retained_anchor_repair_response_seen);
    assert!(checkpoint.retained_anchor_repair_response_id.is_none());

    runtime
        .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
            "cluster-a",
            &identity,
            &anchor_traffic.wire_bytes().expect("traffic anchor wire"),
            &[],
            13,
            &["repository-a".to_owned()],
            "repository-a",
            true,
            Some(RetainedAnchorCheckpointUpdate {
                peer_node_id: "node-b".to_owned(),
                response_id: "second-bounded-response".to_owned(),
                response_complete: true,
                allowance_complete: true,
                streams: BTreeSet::from([runtime_stream, traffic_stream]),
            }),
        )
        .expect("second response consumes the remaining stream allowance");
    assert!(
        runtime
            .initial_peer_backfill_checkpoint("node-b")
            .expect("completed checkpoint")
            .retained_anchor_repair_response_seen
    );
}

fn segment_at_stream(
    signing_key: &ed25519_dalek::SigningKey,
    stream: &str,
    sequence: u64,
    records: Vec<crate::history_sync::SyncRecord>,
    previous: Option<[u8; 32]>,
) -> crate::history_sync::SignedSegment {
    crate::history_sync::CanonicalSegment::new(
        "cluster-a",
        crate::history_sync::Cursor::new("node-a", 7, stream, sequence).expect("cursor"),
        records,
        previous,
        10,
        11,
    )
    .expect("segment")
    .sign(signing_key)
    .expect("signature")
}

#[test]
fn retained_anchor_consumption_survives_gap_ledger_overflow() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    let streams = std::collections::BTreeSet::from([InitialPeerRetainedAnchorStream {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
    }]);
    runtime
        .update_initial_peer_summary_checkpoint_with_retained_anchors(
            "node-b",
            Some("page-2".to_owned()),
            Vec::new(),
            None,
            false,
            false,
            true,
            streams.clone(),
        )
        .expect("persist retained anchor state");
    for index in 0..64 {
        runtime
            .merge_replica_gaps(&[super::RepositoryReplicaGap {
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: format!("gap-{index}"),
                first_sequence: index,
                last_sequence: index,
                start_unix_seconds: 1,
                end_unix_seconds: 1,
                permanent: true,
                reason: None,
            }])
            .expect("persist bounded gap ledger");
    }
    let restored = load(temporary.path());
    let restored_checkpoint = restored
        .initial_peer_backfill_checkpoint("node-b")
        .expect("restored retained anchor checkpoint");
    assert!(restored_checkpoint.retained_anchor_repair_response_seen);
    assert_eq!(restored_checkpoint.retained_anchor_streams, streams);
}

#[test]
fn retained_anchor_consumption_survives_peer_backfill_restart() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    let streams = BTreeSet::from([InitialPeerRetainedAnchorStream {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
    }]);
    runtime
        .update_initial_peer_summary_checkpoint_with_retained_anchors(
            "node-b",
            Some("page-2".to_owned()),
            vec!["segment-1".to_owned()],
            Some("page-3".to_owned()),
            false,
            false,
            true,
            streams.clone(),
        )
        .expect("persist retained anchor state");
    runtime
        .restart_initial_peer_backfill("node-b")
        .expect("restart peer backfill");

    let checkpoint = runtime
        .initial_peer_backfill_checkpoint("node-b")
        .expect("restarted checkpoint");
    assert!(checkpoint.retained_anchor_repair_response_seen);
    assert_eq!(checkpoint.retained_anchor_streams, streams);
    assert!(checkpoint.summary_cursor.is_none());
    assert!(checkpoint.summary_pending_segment_ids.is_empty());
}
