#[test]
fn source_journal_replays_rows_beyond_the_bounded_restart_window() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    for sequence in 0..256_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }
    drop(runtime);
    super::source_delivery_capacity_tests::append_signed_source_journal_rows(
        temporary.path(),
        &key,
        &source_identity,
        "runtime",
        "runtime.v1",
        256..300,
    );

    let mut restored = load(temporary.path());
    for _ in 0..300 {
        let front = restored
            .local_source_pending_segments()
            .into_iter()
            .next()
            .expect("durable backlog row becomes replayable");
        restored
            .acknowledge_local_source_segment(&front.wire)
            .expect("acknowledge durable backlog row");
    }
    assert!(restored.local_source_pending_segments().is_empty());
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    assert!(storage.source_delivery_journal().unwrap().is_empty());
}
