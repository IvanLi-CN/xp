use super::{identity, load, record, segment, signing_key};

#[test]
fn consumed_recovery_seals_the_previously_observed_truncated_gap_once() {
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
    runtime
        .receive_initial_backfill_wire_from_repository(
            "cluster-a",
            &identity,
            &anchor.wire_bytes().expect("anchor wire"),
            12,
            &["repository-a".to_owned()],
            "repository-a",
        )
        .expect_err("ordinary backfill must record the observed gap");
    let additional_gap = super::RepositoryReplicaGap {
        source_node_id: "other-source".to_owned(),
        source_epoch: 7,
        stream: "records".to_owned(),
        first_sequence: 9,
        last_sequence: 10,
        start_unix_seconds: 0,
        end_unix_seconds: 0,
        permanent: false,
        reason: None,
    };
    let second_temporary = tempfile::tempdir().expect("second temporary directory");
    let mut runtime_with_multiple = load(second_temporary.path());
    runtime_with_multiple
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("first wire"),
            11,
        )
        .expect("local progress");
    runtime_with_multiple
        .receive_initial_backfill_wire_from_repository(
            "cluster-a",
            &identity,
            &anchor.wire_bytes().expect("anchor wire"),
            12,
            &["repository-a".to_owned()],
            "repository-a",
        )
        .expect_err("ordinary backfill must record the observed gap");
    runtime_with_multiple
        .merge_replica_gaps_in_memory(&[additional_gap])
        .expect("merge second transient gap");
    assert!(
        !runtime_with_multiple
            .can_accept_consumed_retained_sequence_gap(&anchor.wire_bytes().expect("anchor wire"))
            .expect("reject multiple transient gaps")
    );
    assert!(
        runtime
            .can_accept_consumed_retained_sequence_gap(&anchor.wire_bytes().expect("anchor wire"))
            .expect("probe consumed recovery continuation")
    );

    runtime
        .receive_initial_backfill_wire_from_repository_with_gaps(
            "cluster-a",
            &identity,
            &anchor.wire_bytes().expect("anchor wire"),
            &[],
            13,
            &["repository-a".to_owned()],
            "repository-a",
            true,
        )
        .expect("truncated anchor continuation");
    let checkpoint = runtime
        .receiver
        .as_ref()
        .expect("receiver")
        .continuous_watermarks()[0]
        .clone();
    assert_eq!(checkpoint.sequence(), 3);
    assert!(runtime.snapshot.gaps.iter().any(|gap| {
        gap.first_sequence == 1
            && gap.last_sequence == 2
            && gap.permanent
            && gap.reason.as_deref() == Some("source_retention_expired")
    }));
    assert!(
        !runtime
            .can_accept_consumed_retained_sequence_gap(&anchor.wire_bytes().expect("anchor wire"))
            .expect("second probe")
    );
}
