use super::{
    sync::RepositoryReplicaGap,
    tests::{identity, load, record, segment, signing_key},
    *,
};

#[test]
fn duplicate_repair_segment_clears_a_gap_reintroduced_by_a_peer() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let first_hash = first.segment_hash().expect("hash");
    let second = segment(&key, 1, vec![record(b"two", false)], Some(first_hash));
    let second_hash = second.segment_hash().expect("hash");
    let third = segment(&key, 2, vec![record(b"three", false)], Some(second_hash));
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("first segment");
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &third.wire_bytes().expect("wire"),
            13,
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::SequenceGap {
                expected: 1,
                actual: 2,
            }
        ))
    ));
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &second.wire_bytes().expect("wire"),
            12,
        )
        .expect("repair the missing segment");
    assert!(runtime.snapshot.gaps.is_empty());

    runtime
        .merge_replica_gaps(&[RepositoryReplicaGap {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
            first_sequence: 1,
            last_sequence: 1,
            start_unix_seconds: 12,
            end_unix_seconds: 12,
            permanent: false,
            reason: None,
        }])
        .expect("merge stale peer gap");
    assert_eq!(runtime.snapshot.gaps.len(), 1);

    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &second.wire_bytes().expect("wire"),
            14,
        )
        .expect("duplicate repair segment");
    assert!(runtime.snapshot.gaps.is_empty());

    let restored = load(temporary.path());
    assert!(restored.snapshot.gaps.is_empty());
}

#[test]
fn deep_verification_keeps_a_local_only_gap_incomplete() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let segment = segment(&key, 0, vec![record(b"one", false)], None);
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment.wire_bytes().expect("wire"),
            11,
        )
        .expect("segment");
    let remote = runtime.replication_summary().expect("remote summary");
    runtime
        .merge_replica_gaps(&[RepositoryReplicaGap {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
            first_sequence: 1,
            last_sequence: 1,
            start_unix_seconds: 12,
            end_unix_seconds: 12,
            permanent: false,
            reason: None,
        }])
        .expect("merge local-only gap");

    assert!(
        runtime
            .requires_repair(&remote, true)
            .expect("local-only gap remains incomplete")
    );
}
