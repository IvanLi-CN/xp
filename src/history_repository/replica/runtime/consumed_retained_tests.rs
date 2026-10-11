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
