use super::*;

#[tokio::test]
async fn actual_worker_recovers_evicted_original_through_intermediate_tip_and_real_ack() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let storage = HistoryStorage::open(directory.path());
    let mut wires = prepare(&state, &storage, 96).await;
    let original = storage.source_delivery_journal().unwrap();
    wires.push(RepositoryReplicaSegment {
        identity: original[0].identity.clone(),
        wire: original[0].wire.clone(),
    });
    let key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
    for sequence in 98..240 {
        state
            .repository_replica
            .lock()
            .await
            .queue_local_source_segment(
                &state.cluster.cluster_id,
                original[0].identity.clone(),
                &key,
                vec![sample(&state, sequence)],
                NOW,
            )
            .unwrap();
        // The source API returns its oldest replay row. Select the newly persisted successor.
        let row = storage
            .source_delivery_journal()
            .unwrap()
            .into_iter()
            .find(|row| {
                SignedSegment::from_wire(&row.wire)
                    .unwrap()
                    .canonical()
                    .first_cursor()
                    .sequence()
                    == sequence
            })
            .unwrap();
        let row = RepositoryReplicaSegment {
            identity: row.identity,
            wire: row.wire,
        };
        assert_eq!(
            SignedSegment::from_wire(&row.wire)
                .unwrap()
                .canonical()
                .first_cursor()
                .sequence(),
            sequence
        );
        wires.push(row);
    }
    let mut live_source = control(&storage)["local_source"].clone();
    // Predecessor checkpoints may omit node_id; recovery binds the proven local signer.
    live_source["node_id"] = state.cluster.node_id.clone().into();
    for row in &wires {
        if SignedSegment::from_wire(&row.wire)
            .unwrap()
            .canonical()
            .last_cursor()
            .sequence()
            > 199
        {
            break;
        }
        state
            .repository_replica
            .lock()
            .await
            .receive_wire_from_repository(
                &state.cluster.cluster_id,
                &row.identity,
                &row.wire,
                NOW,
                &[state.cluster.node_id.clone()],
                &state.cluster.node_id,
            )
            .unwrap();
    }
    assert_eq!(storage.repository_history_record_count().unwrap(), 200);
    // Only this test's isolated database loses its retained wire cache; real receiver stays 199.
    let database = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    database
        .execute("DELETE FROM repository_history_segments", [])
        .unwrap();
    drop(database);
    let peer = peer::Peer::start(&state, &wires).await;
    peer.register(&state).await;
    drop(state);
    let mut state = fixture::app_state(directory.path());
    let mut completed = false;
    for step in 0..32 {
        peer.register(&state).await;
        tick(&state, NOW + step * RETRY_SECONDS).await;
        completed = state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_inflight_checkpoint()
            .is_none();
        drop(state);
        state = fixture::app_state(directory.path());
        if completed {
            break;
        }
    }
    assert!(
        completed,
        "actual worker must converge across every tick's restart"
    );
    assert_eq!(storage.repository_history_record_count().unwrap(), 240);
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 142);
    assert_eq!(control(&storage)["local_source"], live_source);
    assert!(control(&storage)["gaps"].as_array().unwrap().is_empty());
    let stored = state
        .repository_replica
        .lock()
        .await
        .repair_batch(&[original[0].id.clone()])
        .unwrap();
    assert!(
        stored.segments.is_empty(),
        "a real historic receipt must not rebuild the expired wire cache"
    );
    assert_eq!(storage.repository_history_segment_count().unwrap(), 40);
    assert!(
        peer.requests()
            .await
            .iter()
            .any(|path| path.ends_with("/repair"))
    );
    peer.register(&state).await;
    tick(&state, NOW + 200).await;
    assert_eq!(storage.repository_history_record_count().unwrap(), 240);
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 142);
}
