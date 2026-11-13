use super::*;

#[test]
fn remote_delivery_does_not_infer_permanent_gaps_from_the_sources_receiver() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).unwrap();
    let key = SigningKey::from_bytes(&[11; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("local".to_owned()).unwrap(),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).unwrap(),
        X25519PublicKey::from_bytes([12; 32]).unwrap(),
    )
    .unwrap();
    let mut queued = Vec::new();
    for sequence in 0..2_u64 {
        queued.push(
            runtime
                .queue_local_source_segment(
                    "cluster-a",
                    identity.clone(),
                    &key,
                    vec![SyncRecord::new(
                        "local",
                        "local",
                        "runtime.v1",
                        1,
                        sequence.to_be_bytes().to_vec(),
                        b"sample".to_vec(),
                        false,
                    )],
                    100 + sequence,
                )
                .unwrap()
                .unwrap(),
        );
    }
    let other_stream = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity.clone(),
            &key,
            vec![SyncRecord::new(
                "local",
                "local",
                "connections.v1",
                1,
                b"other".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            105,
        )
        .unwrap()
        .unwrap();
    runtime
        .receive_wire("cluster-a", &identity, &other_stream.wire, 106)
        .unwrap();
    runtime
        .acknowledge_local_source_segment_via(&other_stream.wire, 107, "direct")
        .unwrap();
    // A former repository lacks its own receiver baseline although the remote collector
    // acknowledged the predecessor. The durable outbox retains only the successor.
    runtime
        .acknowledge_local_source_segment_via(&queued[0].wire, 110, "direct")
        .unwrap();
    let page = runtime.local_source_pending_segments();
    assert_eq!(page.len(), 1);
    runtime.set_local_source_epoch(1, "local");
    let remote =
        super::super::source_delivery::source_gap_page(&mut runtime, "local", "remote", &page, 120)
            .unwrap();
    assert!(
        remote.is_empty(),
        "a local receiver absence proves nothing about a remote receiver"
    );
    let local =
        super::super::source_delivery::source_gap_page(&mut runtime, "local", "local", &page, 120)
            .unwrap();
    assert_eq!(
        local.len(),
        1,
        "local receiver repair remains available for local replay"
    );
    assert_eq!((local[0].first_sequence, local[0].last_sequence), (0, 0));
    runtime
        .record_local_source_delivery_gap(1, "runtime", 0, 0, 121)
        .unwrap();
    let persisted_source_gaps =
        super::super::source_delivery::source_gap_page(&mut runtime, "local", "remote", &page, 122)
            .unwrap();
    assert_eq!(persisted_source_gaps.len(), 1);
    assert!(!persisted_source_gaps[0].permanent);
    assert_eq!(persisted_source_gaps[0].reason, None);

    assert_eq!(
        storage
            .source_delivery_journal_summary()
            .unwrap()
            .pending_segments,
        1
    );
}
