use super::{
    RepositoryRepairBatch,
    tests::{identity, load, record, segment, signing_key},
    *,
};
use crate::history_sync::{CanonicalSegment, Cursor, SyncRecord};
use sha2::Sha256;

#[test]
fn repair_response_digest_binds_the_returned_content() {
    let mut response = RepositoryRepairBatch {
        segments: Vec::new(),
        unavailable_segment_ids: Vec::new(),
        gaps: Vec::new(),
        history_truncated: false,
        retained_stream_boundaries: Vec::new(),
        response_content_digest: None,
        response_id: None,
    };
    let original = response.response_id_digest().expect("response digest");

    response
        .retained_stream_boundaries
        .push(super::RepositoryRetainedStreamBoundary {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
            first_sequence: 0,
        });
    assert_eq!(
        original,
        response
            .response_id_digest()
            .expect("legacy response digest")
    );
    assert_ne!(
        original,
        response
            .response_content_digest()
            .expect("boundary-bound response digest")
    );

    response.history_truncated = true;
    assert_ne!(
        original,
        response
            .response_id_digest()
            .expect("changed response digest")
    );
}

#[test]
fn repair_response_id_accepts_only_the_same_returned_content() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = signing_key();
    let identity = identity(&signing_key);
    let segment = segment(&signing_key, 0, vec![record(b"repair", false)], None);
    let wire = segment.wire_bytes().expect("segment wire");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire("cluster-a", &identity, &wire, 12)
        .expect("store repair segment");
    let segment_id = runtime
        .replication_summary()
        .expect("summary")
        .segment_ids
        .pop()
        .expect("segment id");
    runtime.snapshot.history_truncated = true;

    let response = runtime
        .repair_batch(std::slice::from_ref(&segment_id))
        .expect("repair response");
    assert_eq!(response.retained_stream_boundaries.len(), 1);
    assert_eq!(response.retained_stream_boundaries[0].first_sequence, 0);
    let response_id = response.response_id.clone().expect("response id");
    assert_eq!(
        response.response_id_digest().expect("response digest"),
        response_id
    );

    let mut legacy_response = response.clone();
    legacy_response.response_id = None;
    assert_eq!(
        legacy_response.response_id_digest().expect("legacy digest"),
        response_id
    );
    runtime
        .repair_batch_with_response_id(std::slice::from_ref(&segment_id), Some(&response_id))
        .expect("same response accepted");
    assert!(
        runtime
            .repair_batch_with_response_id(&[segment_id], Some("changed-response"))
            .is_err()
    );
}

#[test]
fn repair_response_splits_pending_segments_at_the_wire_budget() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = signing_key();
    let identity = identity(&signing_key);
    let mut runtime = load(temporary.path());
    let mut previous = None;

    for sequence in 0..64 {
        let segment = CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
            vec![SyncRecord::new(
                "subject-a",
                "node-a",
                "runtime.v1",
                1,
                format!("key-{sequence}").into_bytes(),
                vec![7; 6_000],
                false,
            )],
            previous,
            10,
            11,
        )
        .expect("segment")
        .sign(&signing_key)
        .expect("signature");
        previous = Some(segment.segment_hash().expect("segment hash"));
        runtime
            .receive_wire(
                "cluster-a",
                &identity,
                &segment.wire_bytes().expect("wire"),
                12 + sequence,
            )
            .expect("store segment");
    }

    let requested = runtime.replication_summary().expect("summary").segment_ids;
    assert_eq!(requested.len(), 64);
    let first = runtime.repair_batch(&requested).expect("first repair page");
    assert!(!first.segments.is_empty());
    assert!(first.segments.len() < requested.len());
    assert!(first.unavailable_segment_ids.is_empty());

    let delivered = first
        .segments
        .iter()
        .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
        .collect::<std::collections::BTreeSet<_>>();
    let remaining = requested
        .into_iter()
        .filter(|segment_id| !delivered.contains(segment_id))
        .collect::<Vec<_>>();
    let second = runtime
        .repair_batch(&remaining)
        .expect("remaining repair page");
    assert!(!second.segments.is_empty());
}

#[test]
fn repair_response_rejects_oversized_stored_segment_with_unavailable_id() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = signing_key();
    let identity = identity(&signing_key);
    let segment = segment(&signing_key, 0, vec![record(b"oversized", false)], None);
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = false;
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment.wire_bytes().expect("wire"),
            12,
        )
        .expect("store oversized segment");
    runtime.snapshot.segments[0].wire = vec![0; crate::history_sync::MAX_RESPONSE_WIRE_BYTES];
    let segment_id = runtime
        .replication_summary()
        .expect("summary")
        .segment_ids
        .pop()
        .expect("segment id");

    let error = runtime
        .repair_batch(&[segment_id, "missing-segment".to_owned()])
        .expect_err("oversized retained segment must fail closed");
    assert!(matches!(error, RepositoryRuntimeError::Storage(_)));
}
