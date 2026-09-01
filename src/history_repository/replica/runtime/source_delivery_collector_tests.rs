use super::*;

#[test]
fn source_delivery_unreachable_collector_preserves_backlog() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    let segment = runtime
        .queue_local_source_segment(
            "cluster-a",
            source_identity,
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:unreachable".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            100,
        )
        .expect("queue source segment")
        .expect("source segment")
        .wire;

    for _ in 0..10 {
        runtime
            .record_local_source_collector_delivery("repository-a", "repository-a", false)
            .expect("record unreachable collector")
    }
    assert_eq!(
        runtime.local_source_collector("repository-a", Some("repository-b")),
        "repository-b"
    );
    assert_eq!(runtime.local_source_pending_segments().len(), 1);
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize unreachable collector backlog");
    assert_eq!(summary.pending_segments, 1);
    assert!(summary.last_acknowledged_at.is_none());
    assert!(summary.last_delivery_path.is_none());

    for _ in 0..3 {
        runtime
            .record_local_source_collector_delivery("repository-a", "repository-b", true)
            .expect("record recovered standby collector");
    }
    assert_eq!(
        runtime.local_source_collector("repository-a", Some("repository-b")),
        "repository-a"
    );
    runtime
        .acknowledge_local_source_segment_via(&segment, 123, "direct")
        .expect("acknowledge recovered source segment");
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize drained collector backlog");
    assert_eq!(summary.pending_segments, 0);
    assert_eq!(summary.last_acknowledged_at, Some(123));
    assert_eq!(summary.last_delivery_path.as_deref(), Some("direct"));
}
