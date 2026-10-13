use super::*;

#[test]
fn ready_local_standby_drains_durable_backlog_and_retries_remote_primary() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).unwrap();
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("local".to_owned()).unwrap(),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).unwrap(),
        X25519PublicKey::from_bytes([12; 32]).unwrap(),
    )
    .unwrap();
    for sequence in 0..3_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                identity.clone(),
                &signing_key,
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
            .unwrap();
    }
    for _ in 0..3 {
        runtime
            .record_local_source_collector_delivery("remote", "remote", false)
            .unwrap();
    }
    drop(runtime);
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).unwrap();
    for now in 200..203 {
        let selected = super::super::avoid_local_source_fallback(
            runtime.local_source_collector("remote", Some("local")),
            "remote",
            "local",
            true,
        );
        assert_eq!(selected, "local", "a Ready standby must remain eligible");
        let page = runtime.local_source_pending_segments_page_with_budget(1, 1024 * 1024);
        assert_eq!(page.len(), 1);
        runtime
            .receive_wire("cluster-a", &identity, &page[0].wire, now)
            .unwrap();
        // A lost ACK can replay the same wire through the strict receiver without adding rows.
        runtime
            .receive_wire("cluster-a", &identity, &page[0].wire, now)
            .unwrap();
        runtime
            .acknowledge_local_source_segments_via_without_hydrating(&page, now, "local")
            .unwrap();
        runtime
            .record_local_source_collector_delivery("remote", &selected, true)
            .unwrap();
        drop(runtime);
        runtime = RepositoryReplicaRuntime::load(storage.clone()).unwrap();
        assert_eq!(
            storage
                .source_delivery_journal_summary()
                .unwrap()
                .pending_segments,
            usize::try_from(202 - now).unwrap()
        );
    }
    assert_eq!(
        runtime.local_source_collector("remote", Some("local")),
        "remote"
    );
    assert_eq!(
        storage
            .source_delivery_journal_summary()
            .unwrap()
            .last_acknowledged_at,
        Some(202)
    );
    assert_eq!(
        serde_json::to_value(runtime.runtime_status(203).unwrap()).unwrap()["record_count"],
        3
    );
}

#[test]
fn failed_local_standby_retries_remote_primary_without_acknowledging_backlog() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).unwrap();
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("local".to_owned()).unwrap(),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).unwrap(),
        X25519PublicKey::from_bytes([12; 32]).unwrap(),
    )
    .unwrap();
    let first = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity.clone(),
            &signing_key,
            vec![SyncRecord::new(
                "local",
                "local",
                "runtime.v1",
                1,
                b"first".to_vec(),
                b"original".to_vec(),
                false,
            )],
            100,
        )
        .unwrap()
        .unwrap();
    runtime
        .acknowledge_local_source_segment_via(&first.wire, 101, "direct")
        .unwrap();
    let signed_first = crate::history_sync::SignedSegment::from_wire(&first.wire).unwrap();
    let alternate = CanonicalSegment::new(
        "cluster-a",
        signed_first.canonical().first_cursor().clone(),
        vec![SyncRecord::new(
            "local",
            "local",
            "runtime.v1",
            1,
            b"first".to_vec(),
            b"different retained head".to_vec(),
            false,
        )],
        None,
        100,
        100,
    )
    .unwrap()
    .sign(&signing_key)
    .unwrap()
    .wire_bytes()
    .unwrap();
    runtime
        .receive_wire("cluster-a", &identity, &alternate, 102)
        .unwrap();
    let pending = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity.clone(),
            &signing_key,
            vec![SyncRecord::new(
                "local",
                "local",
                "runtime.v1",
                1,
                b"next".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            103,
        )
        .unwrap()
        .unwrap();
    for _ in 0..3 {
        runtime
            .record_local_source_collector_delivery("remote", "remote", false)
            .unwrap();
    }
    assert_eq!(
        runtime.local_source_collector("remote", Some("local")),
        "local"
    );
    assert!(matches!(
        runtime.receive_wire("cluster-a", &identity, &pending.wire, 104),
        Err(
            crate::state::history_repository::replica::RepositoryRuntimeError::Protocol(
                crate::history_sync::ProtocolError::HashChainMismatch
            )
        )
    ));
    runtime
        .record_local_source_collector_delivery("remote", "local", false)
        .unwrap();
    drop(runtime);
    let runtime = RepositoryReplicaRuntime::load(storage.clone()).unwrap();
    assert_eq!(
        runtime.local_source_collector("remote", Some("local")),
        "remote"
    );
    let journal = storage.source_delivery_journal_summary().unwrap();
    assert_eq!(journal.pending_segments, 1);
    assert_eq!(journal.last_acknowledged_at, Some(101));
    assert_eq!(
        runtime.local_source_pending_segments()[0].wire,
        pending.wire
    );
}
