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
fn late_segment_does_not_clear_a_permanent_gap_as_a_duplicate() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let first_hash = first.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("first segment");
    runtime.snapshot.gaps.push(StoredGap {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_sequence: 1,
        last_sequence: 2,
        start_unix_seconds: 0,
        end_unix_seconds: 11,
        permanent: true,
        reason: Some("test".to_owned()),
    });
    let resumed = segment(&key, 3, vec![record(b"three", false)], Some(first_hash));
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &resumed.wire_bytes().expect("wire"),
            12,
        )
        .expect("segment after permanent gap");
    let late = segment(&key, 1, vec![record(b"late", false)], None);
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &late.wire_bytes().expect("wire"),
            13
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::PermanentGap
        ))
    ));
    assert!(runtime.snapshot.gaps.iter().any(|gap| gap.permanent));
}

#[test]
fn incoming_gaps_are_deduplicated_and_preserved_over_a_full_local_ledger() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let first = segment(&key, 0, vec![record(b"first", false)], None);
    let first_hash = first.segment_hash().expect("first segment hash");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("first segment wire"),
            1,
        )
        .expect("seed receiver watermark");
    let local = (0..64_u64)
        .map(|sequence| RepositoryReplicaGap {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: format!("stream-{sequence:02}"),
            first_sequence: sequence,
            last_sequence: sequence,
            start_unix_seconds: 1,
            end_unix_seconds: 1,
            permanent: sequence == 0,
            reason: None,
        })
        .collect::<Vec<_>>();
    runtime
        .merge_replica_gaps(&local)
        .expect("seed local gap ledger");

    let required = RepositoryReplicaGap {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_sequence: 1,
        last_sequence: 2,
        start_unix_seconds: 2,
        end_unix_seconds: 2,
        permanent: true,
        reason: None,
    };
    let transient = RepositoryReplicaGap {
        permanent: false,
        ..required.clone()
    };
    let recoverable = RepositoryReplicaGap {
        stream: "recoverable".to_owned(),
        first_sequence: 100,
        last_sequence: 100,
        permanent: false,
        ..required.clone()
    };
    runtime
        .merge_replica_gaps(&[transient, required.clone(), recoverable.clone()])
        .expect("merge authenticated source gaps");

    assert_eq!(runtime.snapshot.gaps.len(), 64);
    assert_eq!(
        runtime
            .snapshot
            .gaps
            .iter()
            .filter(|gap| {
                gap.source_node_id == required.source_node_id
                    && gap.source_epoch == required.source_epoch
                    && gap.stream == required.stream
                    && gap.first_sequence == required.first_sequence
                    && gap.last_sequence == required.last_sequence
            })
            .count(),
        1
    );
    assert!(runtime.snapshot.gaps.iter().any(|gap| {
        gap.stream == required.stream
            && gap.first_sequence == required.first_sequence
            && gap.last_sequence == required.last_sequence
            && gap.permanent
    }));
    assert!(
        runtime
            .snapshot
            .gaps
            .iter()
            .any(|gap| { gap.stream == "stream-00" && gap.first_sequence == 0 && gap.permanent })
    );
    assert!(runtime.snapshot.gaps.iter().any(|gap| {
        gap.stream == recoverable.stream
            && gap.first_sequence == recoverable.first_sequence
            && !gap.permanent
    }));

    let resumed = segment(&key, 3, vec![record(b"resumed", false)], Some(first_hash));
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &resumed.wire_bytes().expect("resumed segment wire"),
            2,
        )
        .expect("segment after the preserved source gap");
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
