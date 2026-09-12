use super::{identity, load, record, segment, signing_key};
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
