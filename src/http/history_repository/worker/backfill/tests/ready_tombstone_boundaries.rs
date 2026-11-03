use super::*;
use crate::{
    history_sync::SignedSegment,
    http::history_repository::worker::backfill::tests::ready_tombstones::{membership, stage},
    internal_auth::InternalRoute,
    state::{
        history_repository::{
            identity::RepositoryNodeIdentity,
            replica::{RepositoryReplicaGap, RepositoryReplicaRuntime, RepositoryReplicaSegment},
        },
        history_storage::REPOSITORY_REPLICA_KEY,
    },
};
use sha2::{Digest, Sha256};

#[path = "app_state.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "ready_tombstone_peer.rs"]
mod peer;
const NOW: u64 = 1_800_000_000;
const RESPONSE_BYTES: usize = 2 * 1024 * 1024;

fn journal(storage: &HistoryStorage) -> serde_json::Value {
    serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap()
}

fn empty_barrier(epoch: u64) -> Barrier {
    Barrier {
        version: 1,
        epoch,
        page_digest: String::new(),
        source_digest: String::new(),
        protected: Vec::new(),
        peers: BTreeMap::new(),
        next_peer: 0,
        finished: false,
    }
}

#[tokio::test]
async fn self_signed_or_other_source_pending_wire_cannot_create_a_protected_stream() {
    for forged_local_id in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let state = fixture::app_state(directory.path());
        let wires = stage(&state).await;
        let server = peer::Peer::start(&state, &wires).await;
        server.register(&state).await;
        membership(&state).await;
        let other = derived_repository_identity(
            &state,
            RepositoryNodeId::try_from("other-source".to_owned()).unwrap(),
        )
        .unwrap();
        let identity = if forged_local_id {
            RepositoryNodeIdentity::new(
                RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
                other.ed25519_public_key().clone(),
                other.x25519_relay_public_key().clone(),
            )
            .unwrap()
        } else {
            other
        };
        let pending = SignedSegment::from_wire(&wires[1].wire).unwrap();
        let first = pending.canonical().first_cursor();
        let forged = CanonicalSegment::new(
            &state.cluster.cluster_id,
            Cursor::new(
                identity.node_id().as_str(),
                first.source_epoch(),
                "traffic",
                2,
            )
            .unwrap(),
            pending.canonical().records().to_vec(),
            Some([1; 32]),
            NOW,
            NOW,
        )
        .unwrap()
        .sign(&derived_repository_signing_key(&state, "other-source").unwrap())
        .unwrap();
        // The signature is valid under the identity embedded in the corrupt checkpoint.
        forged.verify_identity(&identity).unwrap();
        let wire = forged.wire_bytes().unwrap();
        let id = hex::encode(Sha256::digest(&wire));
        let storage = HistoryStorage::open(directory.path());
        let mut snapshot: serde_json::Value =
            serde_json::from_slice(&storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap())
                .unwrap();
        snapshot["local_history_backfill_inflight"] = serde_json::json!({
            "segment_ids": [id], "page_cursor": null, "completed": true,
            "segments": [{"id": id, "identity": identity, "wire": wire}],
        });
        storage
            .write(
                REPOSITORY_REPLICA_KEY,
                &serde_json::to_vec(&snapshot).unwrap(),
            )
            .unwrap();
        *state.repository_replica.lock().await =
            RepositoryReplicaRuntime::load(storage.clone()).unwrap();
        let before = storage.read(REPOSITORY_REPLICA_KEY).unwrap();
        let original_journal = journal(&storage);
        let error = before_local(&state, NOW).await.unwrap_err();
        assert!(
            error.to_string().contains("local pinned identity"),
            "{error:#}"
        );
        assert!(server.requests().await.is_empty());
        assert!(load(&state).unwrap().is_none());
        assert_eq!(storage.read(REPOSITORY_REPLICA_KEY).unwrap(), before);
        assert_eq!(journal(&storage), original_journal);
    }
}

#[tokio::test]
async fn protected_receiver_stream_rejects_cross_epoch_gaps_and_retained_anchors() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    before_local(&state, NOW).await.unwrap();
    let pending = SignedSegment::from_wire(&wires[1].wire).unwrap();
    let cursor = pending.canonical().first_cursor();
    for epoch in [cursor.source_epoch(), cursor.source_epoch() + 1] {
        let anchor = CanonicalSegment::new(
            &state.cluster.cluster_id,
            Cursor::new(cursor.source_node_id(), epoch, cursor.stream(), 9).unwrap(),
            pending.canonical().records().to_vec(),
            Some([9; 32]),
            NOW,
            NOW,
        )
        .unwrap()
        .sign(&derived_repository_signing_key(&state, &state.cluster.node_id).unwrap())
        .unwrap();
        let segment = RepositoryReplicaSegment {
            identity: wires[1].identity.clone(),
            wire: anchor.wire_bytes().unwrap(),
        };
        assert!(super::super::protected::wire_is_protected(&state, &segment.wire).unwrap());
        let storage = HistoryStorage::open(directory.path());
        let before = storage.read(REPOSITORY_REPLICA_KEY).unwrap();
        let original_journal = journal(&storage);
        let result = super::super::protected::receive(&state, &segment, &[], NOW, &[]).await;
        assert!(!matches!(result, Ok(true)));
        assert_eq!(storage.read(REPOSITORY_REPLICA_KEY).unwrap(), before);
        assert_eq!(journal(&storage), original_journal);
    }
    let mut gaps = vec![RepositoryReplicaGap {
        source_node_id: cursor.source_node_id().into(),
        source_epoch: cursor.source_epoch() + 1,
        stream: cursor.stream().into(),
        first_sequence: 0,
        last_sequence: 0,
        start_unix_seconds: NOW,
        end_unix_seconds: NOW,
        permanent: true,
        reason: Some("source_retention_expired".into()),
    }];
    assert!(super::super::protected::filter_gaps(&state, &mut gaps).is_err());
    assert_eq!(gaps.len(), 1);
    drop(state);
    let state = fixture::app_state(directory.path());
    assert!(super::super::protected::filter_gaps(&state, &mut gaps).is_err());
}

#[tokio::test]
async fn ready_buffer_has_an_aggregate_limit_and_quota_and_filesystem_admission() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let mut barrier = empty_barrier(0);
    save(&state, &barrier).unwrap();
    let baseline = storage(&state).unwrap().read(KEY).unwrap();
    let page = URL_SAFE_NO_PAD.encode(vec![
        0;
        super::super::super::MAX_INITIAL_BACKFILL_WIRE_BYTES
    ]);
    for index in 0..5 {
        barrier.peers.insert(
            index.to_string(),
            PeerPhase {
                buffer: Some(page.clone()),
                ..Default::default()
            },
        );
    }
    assert!(
        save_buffer(&state, &barrier)
            .await
            .unwrap_err()
            .to_string()
            .contains("aggregate")
    );
    assert_eq!(storage(&state).unwrap().read(KEY).unwrap(), baseline);
    barrier.peers.clear();
    barrier.peers.insert(
        "peer".into(),
        PeerPhase {
            buffer: Some(URL_SAFE_NO_PAD.encode(b"bounded page")),
            ..Default::default()
        },
    );
    let quota = state
        .repository_replica
        .lock()
        .await
        .runtime_capacity()
        .unwrap()
        .quota_bytes();
    for (used, available) in [(quota - 1, u64::MAX), (0, 1)] {
        state
            .repository_replica
            .lock()
            .await
            .force_capacity_for_test(used, available)
            .unwrap();
        assert!(
            save_buffer(&state, &barrier)
                .await
                .unwrap_err()
                .to_string()
                .contains("capacity")
        );
        assert_eq!(storage(&state).unwrap().read(KEY).unwrap(), baseline);
    }
    state
        .repository_replica
        .lock()
        .await
        .force_capacity_for_test(0, u64::MAX)
        .unwrap();
    save_buffer(&state, &barrier).await.unwrap();
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        load(&state).unwrap().unwrap().peers["peer"].buffer,
        barrier.peers["peer"].buffer
    );
}

#[tokio::test]
async fn buffered_page_replay_after_rows_commit_is_idempotent_and_reclaims_checkpoint_space() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires[..3]).await;
    server.register(&state).await;
    membership(&state).await;
    assert_eq!(
        before_local(&state, NOW).await.unwrap(),
        InitialBackfillProgress::Complete
    );
    for step in 0..20 {
        crate::http::history_repository::worker::advance_local_repository_lifecycle(
            &state,
            NOW + step * 5,
        )
        .await
        .unwrap();
        if state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
        {
            break;
        }
    }
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
    let barrier_before = storage(&state).unwrap().read(KEY).unwrap().unwrap();
    let original_journal = journal(&storage(&state).unwrap());
    let database = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    // Fail only the final marker transaction, after actual row and runtime cursor commits.
    database
        .execute_batch(
            "CREATE TRIGGER fail_ready_marker BEFORE UPDATE ON history_snapshots
         WHEN NEW.key = 'repository_ready_tombstone_barrier_v1'
         BEGIN SELECT RAISE(ABORT, 'injected Ready checkpoint failure'); END;",
        )
        .unwrap();
    assert!(after_local(&state, NOW).await.is_err());
    let persisted: Vec<u8> = database
        .query_row(
            "SELECT payload FROM history_snapshots WHERE key = ?1",
            [KEY],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted, barrier_before);
    database
        .execute_batch("DROP TRIGGER fail_ready_marker")
        .unwrap();
    drop(database);
    drop(state);
    let state = fixture::app_state(directory.path());
    let rows = storage(&state)
        .unwrap()
        .repository_history_record_count()
        .unwrap();
    // The verified live suffix is now received before the original journal ACK.
    assert_eq!(rows, 5);
    let records = storage(&state)
        .unwrap()
        .repository_history_records(None, None, None, 0, 16)
        .unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|row| row.record_key == b"node-history:user:suffix:current")
            .count(),
        1
    );
    after_local(&state, NOW + 5).await.unwrap();
    assert_eq!(
        storage(&state)
            .unwrap()
            .repository_history_record_count()
            .unwrap(),
        rows
    );
    assert_eq!(journal(&storage(&state).unwrap()), original_journal);
    let barrier = load(&state).unwrap().unwrap();
    assert!(
        barrier
            .peers
            .values()
            .all(|phase| phase.prepared && phase.buffer.is_none())
    );
    assert_eq!(
        storage(&state)
            .unwrap()
            .source_delivery_journal()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn aggregate_buffer_pressure_restarts_records_at_the_first_page_after_original_ack() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires[..3]).await;
    server.register(&state).await;
    membership(&state).await;
    assert_eq!(
        before_local(&state, NOW).await.unwrap(),
        InitialBackfillProgress::Complete
    );
    let mut barrier = load(&state).unwrap().unwrap();
    let phase = barrier.peers.values_mut().next().unwrap();
    let page = URL_SAFE_NO_PAD
        .decode(phase.buffer.as_ref().unwrap())
        .unwrap();
    let next = phase.cursor.clone();
    assert!(next.is_some());
    // Exercise the same cache-pressure policy with the real signed peer's first records page.
    hold_first_records_page(phase, &page, next, 0);
    assert!(phase.buffer.is_none() && phase.cursor.is_none() && !phase.terminal);
    assert!(phase.deletions_complete);
    save(&state, &barrier).unwrap();
    assert_eq!(
        storage(&state)
            .unwrap()
            .repository_history_record_count()
            .unwrap(),
        0
    );
    assert_eq!(
        storage(&state)
            .unwrap()
            .source_delivery_journal()
            .unwrap()
            .len(),
        2
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    for step in 0..20 {
        crate::http::history_repository::worker::advance_local_repository_lifecycle(
            &state,
            NOW + step * 5,
        )
        .await
        .unwrap();
        if state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
        {
            break;
        }
    }
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
    assert_eq!(
        storage(&state)
            .unwrap()
            .source_delivery_journal()
            .unwrap()
            .len(),
        1
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    for step in 20..45 {
        crate::http::history_repository::worker::advance_local_repository_lifecycle(
            &state,
            NOW + step * 5,
        )
        .await
        .unwrap();
        if state
            .repository_replica
            .lock()
            .await
            .initial_peer_backfill_checkpoint("registered-syncing-peer")
            .is_some_and(|checkpoint| checkpoint.completed && checkpoint.summary_complete)
        {
            break;
        }
    }
    assert_eq!(
        storage(&state)
            .unwrap()
            .repository_history_record_count()
            .unwrap(),
        5
    );
    assert_eq!(
        storage(&state)
            .unwrap()
            .source_delivery_journal()
            .unwrap()
            .len(),
        1
    );
    assert!(load(&state).unwrap().unwrap().finished);
}

#[tokio::test]
async fn same_epoch_page_replacement_and_new_source_stream_expand_the_durable_guard_after_restart()
{
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    for step in 0..20 {
        crate::http::history_repository::worker::advance_local_repository_lifecycle(
            &state,
            NOW + step * 5,
        )
        .await
        .unwrap();
        if state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
        {
            break;
        }
    }
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
    let previous = load(&state).unwrap().unwrap();
    assert_eq!(previous.page_digest.len(), 64);
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
    let record = |schema, name: &[u8]| {
        SyncRecord::new(
            &state.cluster.node_id,
            &state.cluster.node_id,
            schema,
            1,
            name.to_vec(),
            b"new same-epoch page".to_vec(),
            false,
        )
    };
    let mut runtime = state.repository_replica.lock().await;
    let epoch = runtime.local_history_backfill_collection_state().3;
    let page = runtime
        .queue_local_history_backfill_batches_at(
            &state.cluster.cluster_id,
            identity.clone(),
            &key,
            None,
            true,
            vec![(
                vec![record("runtime.v1", b"new-independent-page")],
                NOW + 100,
            )],
            NOW + 100,
        )
        .unwrap();
    runtime
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity,
            &key,
            vec![record("connections.v1", b"new-live-stream")],
            NOW + 100,
        )
        .unwrap();
    assert_eq!(runtime.local_history_backfill_collection_state().3, epoch);
    drop(runtime);
    let new_page = SignedSegment::from_wire(&page[0].wire).unwrap();
    let stream = new_page.canonical().first_cursor().stream();
    let journal_before = journal(&storage(&state).unwrap());
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        before_local(&state, NOW + 105).await.unwrap(),
        InitialBackfillProgress::Complete
    );
    let rebound = load(&state).unwrap().unwrap();
    assert_eq!(rebound.epoch, previous.epoch);
    assert_ne!(rebound.page_digest, previous.page_digest);
    assert_ne!(rebound.source_digest, previous.source_digest);
    for old in previous.protected {
        assert!(
            rebound
                .protected
                .iter()
                .any(|entry| entry.source == old.source
                    && entry.epoch == old.epoch
                    && entry.stream == old.stream
                    && entry.last >= old.last)
        );
    }
    assert!(rebound.protected.iter().any(|entry| entry.stream == stream));
    assert!(
        rebound
            .protected
            .iter()
            .any(|entry| entry.stream == "connections" && entry.last.is_none())
    );
    assert_eq!(journal(&storage(&state).unwrap()), journal_before);
    assert!(
        after_local(&state, NOW + 105).await.is_err(),
        "records cannot race the replacement page"
    );
    crate::http::history_repository::worker::advance_local_repository_lifecycle(&state, NOW + 110)
        .await
        .unwrap();
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
    let acknowledged = load(&state).unwrap().unwrap();
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    assert_ne!(acknowledged.page_digest, rebound.page_digest);
    assert_eq!(journal(&storage(&state).unwrap()), journal_before);
    drop(state);
    let state = fixture::app_state(directory.path());
    refresh(&state).await.unwrap();
    let restored = load(&state).unwrap().unwrap();
    assert_eq!(restored.page_digest, acknowledged.page_digest);
    assert!(
        restored
            .protected
            .iter()
            .any(|entry| entry.stream == stream)
    );
    assert!(super::super::protected::wire_is_protected(&state, &page[0].wire).unwrap());
    let mut runtime = state.repository_replica.lock().await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    runtime
        .apply_local_history_backfill_page(
            &state.cluster.cluster_id,
            identity,
            &key,
            None,
            true,
            vec![(
                vec![SyncRecord::new(
                    &state.cluster.node_id,
                    &state.cluster.node_id,
                    "traffic.v1",
                    1,
                    b"instant-page".to_vec(),
                    b"instant payload".to_vec(),
                    false,
                )],
                NOW + 120,
            )],
            NOW + 120,
            &[],
            &state.cluster.node_id,
        )
        .unwrap();
    assert!(
        runtime
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    drop(runtime);
    refresh(&state).await.unwrap();
    let instant = load(&state).unwrap().unwrap();
    assert_ne!(instant.source_digest, restored.source_digest);
    assert!(instant.protected.iter().any(|entry| entry.stream
        == format!("traffic-backfill-{}", state.cluster.node_id)
        && entry.last == Some(0)));
    assert_eq!(journal(&storage(&state).unwrap()), journal_before);
}
