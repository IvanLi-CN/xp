use super::*;

#[test]
fn syncing_tombstone_receipt_is_deferred_until_repository_is_ready() {
    let source_dir = tempfile::tempdir().expect("source directory");
    let collector_dir = tempfile::tempdir().expect("collector directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let tombstone = SyncRecord::new(
        "node-a",
        "node-a",
        "traffic.v1",
        1,
        b"node-history:node:node-a:".to_vec(),
        b"deleted".to_vec(),
        true,
    );
    let mut source = load(source_dir.path());
    let queued = source
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            source_identity.clone(),
            &key,
            vec![tombstone],
            100,
            &["repo-a".to_owned(), "repo-b".to_owned()],
        )
        .expect("queue tombstone");
    let mut collector = load(collector_dir.path());
    let syncing_receipt = collector
        .receive_wire_from_repository(
            "cluster-a",
            &source_identity,
            &queued[0].wire,
            100,
            &["repo-a".to_owned()],
            "repo-b",
        )
        .expect("syncing collector accepts tombstone");
    assert!(syncing_receipt.tombstone_acknowledgements().is_empty());

    let ready_receipt = collector
        .receive_wire_from_repository(
            "cluster-a",
            &source_identity,
            &queued[0].wire,
            101,
            &["repo-a".to_owned(), "repo-b".to_owned()],
            "repo-b",
        )
        .expect("ready collector replays tombstone acknowledgement");
    assert_eq!(ready_receipt.tombstone_acknowledgements().len(), 1);
}

#[test]
fn source_backlog_does_not_become_a_permanent_gap_when_delivery_is_delayed() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    for sequence in 0..9_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
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
            .expect("queue delayed source segment");
    }

    assert!(runtime.local_source_backpressure_gaps("node-a").is_empty());
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 9);
    assert_eq!(runtime.local_source_next_sequence("runtime"), Some(9));

    let front = runtime
        .local_source_pending_segments()
        .into_iter()
        .next()
        .expect("front source segment");
    runtime
        .acknowledge_local_source_segment(&front.wire)
        .expect("acknowledge source segment");
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 8);
    drop(runtime);
    let restored = load(temporary.path());
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 8);
    assert_eq!(restored.local_source_next_sequence("runtime"), Some(9));
    assert_eq!(restored.local_source_pending_segments().len(), 1);
}
