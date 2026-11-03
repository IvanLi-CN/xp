use super::super::*;
use crate::{
    history_sync::SignedSegment,
    internal_auth::InternalRoute,
    state::{
        history_repository::{
            HistoryStorage,
            control::{RepositoryCapacity, RepositoryMember, RepositoryMembership},
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
const PEER: &str = "registered-syncing-peer";

fn persisted_tombstone_count(directory: &std::path::Path) -> i64 {
    rusqlite::Connection::open(directory.join("history.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM repository_history_records WHERE is_tombstone = 1",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

async fn make_receiver_stale(state: &AppState) {
    let source = derived_repository_identity(
        state,
        RepositoryNodeId::try_from("retained-source".to_owned()).unwrap(),
    )
    .unwrap();
    let wire = CanonicalSegment::new(
        &state.cluster.cluster_id,
        Cursor::new("retained-source", 7, "traffic", 0).unwrap(),
        vec![SyncRecord::new(
            "retained",
            "retained",
            "traffic.v1",
            1,
            b"retained-key".to_vec(),
            b"valid".to_vec(),
            false,
        )],
        None,
        10,
        10,
    )
    .unwrap()
    .sign(&derived_repository_signing_key(state, "retained-source").unwrap())
    .unwrap()
    .wire_bytes()
    .unwrap();
    state
        .repository_replica
        .lock()
        .await
        .receive_wire(&state.cluster.cluster_id, &source, &wire, 10)
        .unwrap();
    let storage = HistoryStorage::open(&state.config.data_dir);
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap()).unwrap();
    snapshot["last_verified_unix_seconds"] = serde_json::json!(1);
    storage
        .write(
            REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    *state.repository_replica.lock().await = RepositoryReplicaRuntime::load(storage).unwrap();
}

#[tokio::test]
async fn stale_ready_barrier_never_waits_for_runtime_while_holding_store() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    make_receiver_stale(&state).await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let runtime_guard = state.repository_replica.lock().await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        super::super::ordering::retained_stale_before_local(&state, &identity, NOW),
    )
    .await
    .expect("worker waited for runtime while holding store")
    .unwrap();
    assert!(result.is_none());
    let store_guard = tokio::time::timeout(std::time::Duration::from_secs(2), state.store.lock())
        .await
        .expect("worker retained the store during contention");
    drop(store_guard);
    drop(runtime_guard);
}

#[tokio::test]
async fn stale_ready_worker_recovers_original_after_restart_without_losing_journal() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    make_receiver_stale(&state).await;
    let storage = HistoryStorage::open(directory.path());
    let original = storage.source_delivery_journal().unwrap();
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let baseline = super::super::ordering::retained_stale_before_local(&state, &identity, NOW)
        .await
        .unwrap();
    assert!(baseline.is_some());
    assert_eq!(
        serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    for step in 0..45 {
        super::super::ordering::advance_initial_catch_up(&state, NOW + step * 5, false, true)
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
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 1);
    assert!(
        storage
            .repository_history_records(None, None, None, 0, 32)
            .unwrap()
            .iter()
            .any(|row| row.record_key == b"retained-key" && !row.tombstone)
    );
    assert!(
        server
            .requests()
            .await
            .iter()
            .any(|path| path.contains("initial-backfill"))
    );
}

#[tokio::test]
async fn stale_ready_worker_rolls_back_page_and_cursor_on_sqlite_failure() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    make_receiver_stale(&state).await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let binding = {
        let store = state.store.lock().await;
        super::super::ordering::retained_ready(&state, &store)
            .unwrap()
            .binding
    };
    let before = {
        let mut runtime = state.repository_replica.lock().await;
        runtime
            .begin_retained_stale_recovery(&state.cluster.cluster_id, &identity, &binding, NOW)
            .unwrap();
        runtime
            .retained_stale_recovery_progress(&state.cluster.cluster_id, &identity, &binding)
            .unwrap()
    };
    let storage = HistoryStorage::open(directory.path());
    let journal = serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_stale_deletion BEFORE INSERT ON repository_history_records
        WHEN NEW.is_tombstone = 1 BEGIN SELECT RAISE(ABORT, 'test failure'); END;",
        )
        .unwrap();
    assert!(
        super::super::ordering::retained_stale_before_local(&state, &identity, NOW)
            .await
            .is_err()
    );
    let after = state
        .repository_replica
        .lock()
        .await
        .retained_stale_recovery_progress(&state.cluster.cluster_id, &identity, &binding)
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(storage.repository_history_record_count().unwrap(), 1);
    assert_eq!(
        serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
        journal
    );
    connection
        .execute_batch("DROP TRIGGER fail_stale_deletion")
        .unwrap();
    assert!(
        super::super::ordering::retained_stale_before_local(&state, &identity, NOW)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn stale_ready_worker_rechecks_member_before_original_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    make_receiver_stale(&state).await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let (peers, generation, ready) =
        super::super::ordering::retained_stale_before_local(&state, &identity, NOW)
            .await
            .unwrap()
            .unwrap();
    let storage = HistoryStorage::open(directory.path());
    let journal = serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap();
    {
        let mut store = state.store.lock().await;
        let mut node = store.get_node(PEER).unwrap();
        node.api_base_url = "https://changed.example.invalid".into();
        store.upsert_node(node).unwrap();
    }
    assert!(
        super::super::legacy_recovery::recover_legacy_original_page_with_retained_baseline(
            &state, &identity, NOW, &ready, &peers, generation
        )
        .await
        .is_err()
    );
    assert_eq!(
        serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
        journal
    );
    assert!(
        !state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
}

#[tokio::test]
async fn stale_first_repository_without_ready_peer_stays_retryable() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    stage(&state).await;
    make_receiver_stale(&state).await;
    let storage = HistoryStorage::open(directory.path());
    let before = serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap();
    for now in [NOW, NOW + 5] {
        assert_eq!(
            super::super::ordering::advance_initial_catch_up(&state, now, false, false)
                .await
                .unwrap(),
            InitialBackfillProgress::Unavailable
        );
    }
    assert_eq!(
        serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
        before
    );
    assert_eq!(storage.repository_history_record_count().unwrap(), 1);
    assert!(
        !state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_completed()
    );
}

#[tokio::test]
async fn stale_ready_worker_restarts_expired_cursor_without_duplicate_deletions() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    make_receiver_stale(&state).await;
    server.fault(peer::Fault::Expired).await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    assert!(
        super::super::ordering::retained_stale_before_local(&state, &identity, NOW)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        super::super::ordering::retained_stale_before_local(&state, &identity, NOW + 5)
            .await
            .unwrap()
            .is_some()
    );
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(persisted_tombstone_count(directory.path()), 2);
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 2);
}

#[tokio::test]
async fn concurrent_stale_ready_workers_retry_without_skipping_pages() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    make_receiver_stale(&state).await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let (left, right) = tokio::join!(
        super::super::ordering::retained_stale_before_local(&state, &identity, NOW),
        super::super::ordering::retained_stale_before_local(&state, &identity, NOW),
    );
    left.unwrap();
    right.unwrap();
    assert!(
        super::super::ordering::retained_stale_before_local(&state, &identity, NOW + 5)
            .await
            .unwrap()
            .is_some()
    );
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(persisted_tombstone_count(directory.path()), 2);
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 2);
}

pub(in crate::http::history_repository::worker::backfill) async fn membership(state: &AppState) {
    let local = derived_repository_identity(
        state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let remote =
        derived_repository_identity(state, RepositoryNodeId::try_from(PEER.to_owned()).unwrap())
            .unwrap();
    let members = vec![local, remote]
        .into_iter()
        .map(|id| RepositoryMember::new(id, RepositoryCapacity::default()).unwrap())
        .collect();
    let mut membership = RepositoryMembership::new(members).unwrap();
    let id = RepositoryNodeId::try_from(PEER.to_owned()).unwrap();
    membership.mark_catch_up_complete(&id, NOW - 300).unwrap();
    membership.mark_ready(&id, NOW).unwrap();
    let mut store = state.store.lock().await;
    store.state_mut().repository_membership = Some(membership);
    store.save().unwrap();
}

pub(in crate::http::history_repository::worker::backfill) async fn stage(
    state: &AppState,
) -> Vec<RepositoryReplicaSegment> {
    let local = &state.cluster.node_id;
    let identity =
        derived_repository_identity(state, RepositoryNodeId::try_from(local.clone()).unwrap())
            .unwrap();
    let key = derived_repository_signing_key(state, local).unwrap();
    let record = |key: String| {
        SyncRecord::new(
            local,
            local,
            "traffic.v1",
            1,
            key.into_bytes(),
            b"payload".to_vec(),
            false,
        )
    };
    let mut runtime = state.repository_replica.lock().await;
    let prefix = runtime
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity.clone(),
            &key,
            vec![
                record("node-history:user:prefix-a:current".into()),
                record("node-history:user:prefix-b:current".into()),
            ],
            NOW,
        )
        .unwrap()
        .unwrap();
    runtime
        .acknowledge_local_source_segment_via(&prefix.wire, NOW, "direct")
        .unwrap();
    let pending = runtime
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity.clone(),
            &key,
            vec![
                record(format!("node-history:node:{local}:old")),
                record("node-history:user:keep:current".into()),
            ],
            NOW,
        )
        .unwrap()
        .unwrap();
    runtime
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity,
            &key,
            vec![record("node-history:user:suffix:current".into())],
            NOW,
        )
        .unwrap()
        .unwrap();
    drop(runtime);
    let storage = HistoryStorage::open(&state.config.data_dir);
    let suffix = storage
        .source_delivery_journal()
        .unwrap()
        .into_iter()
        .find(|row| {
            SignedSegment::from_wire(&row.wire)
                .unwrap()
                .canonical()
                .first_cursor()
                .sequence()
                == 4
        })
        .unwrap();
    let suffix = RepositoryReplicaSegment {
        identity: suffix.identity,
        wire: suffix.wire,
    };
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap()).unwrap();
    snapshot["local_history_backfill_inflight"] = serde_json::json!({
        "segment_ids": [hex::encode(Sha256::digest(&pending.wire))],
        "page_cursor": null, "completed": true,
    });
    storage
        .write(
            REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    *state.repository_replica.lock().await = RepositoryReplicaRuntime::load(storage).unwrap();
    let delete_identity = derived_repository_identity(
        state,
        RepositoryNodeId::try_from("deletion-source".to_owned()).unwrap(),
    )
    .unwrap();
    let deletes = CanonicalSegment::new(
        &state.cluster.cluster_id,
        Cursor::new("deletion-source", 7, "tombstone", 0).unwrap(),
        vec![
            SyncRecord::new(
                "!other",
                "!other",
                "traffic.v1",
                1,
                b"node-history:node:!other:".to_vec(),
                Vec::new(),
                true,
            ),
            SyncRecord::new(
                local,
                local,
                "traffic.v1",
                1,
                format!("node-history:node:{local}:").into_bytes(),
                Vec::new(),
                true,
            ),
        ],
        None,
        NOW,
        NOW,
    )
    .unwrap()
    .sign(&derived_repository_signing_key(state, "deletion-source").unwrap())
    .unwrap();
    vec![
        prefix,
        pending,
        suffix,
        RepositoryReplicaSegment {
            identity: delete_identity,
            wire: deletes.wire_bytes().unwrap(),
        },
    ]
}

#[tokio::test]
async fn ready_without_tombstones_buffers_first_records_page_until_original_ack() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires[..3]).await;
    server.register(&state).await;
    membership(&state).await;
    assert_eq!(
        super::super::ready_peer::tombstones::before_local(&state, NOW)
            .await
            .unwrap(),
        InitialBackfillProgress::Complete
    );
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(storage.repository_history_record_count().unwrap(), 0);
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 2);
    drop(state);
    let state = fixture::app_state(directory.path());
    for step in 1..45 {
        super::super::super::advance_local_repository_lifecycle(&state, NOW + step * 5)
            .await
            .unwrap();
        if state
            .repository_replica
            .lock()
            .await
            .initial_peer_backfill_checkpoint(PEER)
            .is_some_and(|checkpoint| checkpoint.completed && checkpoint.summary_complete)
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
    assert_eq!(storage.repository_history_record_count().unwrap(), 5);
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 1);
}

#[tokio::test]
async fn protected_retained_anchor_waits_without_creating_a_permanent_gap() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    for step in 0..20 {
        super::super::super::advance_local_repository_lifecycle(&state, NOW + step * 5)
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
    let suffix = SignedSegment::from_wire(&wires[2].wire).unwrap();
    let cursor = suffix.canonical().first_cursor();
    let anchor = CanonicalSegment::new(
        &state.cluster.cluster_id,
        Cursor::new(cursor.source_node_id(), cursor.source_epoch(), "traffic", 9).unwrap(),
        suffix.canonical().records().to_vec(),
        Some([9; 32]),
        NOW,
        NOW,
    )
    .unwrap()
    .sign(&derived_repository_signing_key(&state, &state.cluster.node_id).unwrap())
    .unwrap();
    let segment = RepositoryReplicaSegment {
        identity: wires[2].identity.clone(),
        wire: anchor.wire_bytes().unwrap(),
    };
    let storage = HistoryStorage::open(directory.path());
    let before = storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap();
    assert!(
        !super::super::ready_peer::protected::receive(
            &state,
            &segment,
            &[],
            NOW + 100,
            &[PEER.to_owned()]
        )
        .await
        .unwrap()
    );
    assert_eq!(
        storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap(),
        before
    );
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .replication_summary()
            .unwrap()
            .gaps
            .is_empty()
    );
    assert!(
        super::super::ready_peer::protected::receive(
            &state,
            &wires[2],
            &[],
            NOW + 101,
            &[PEER.to_owned()]
        )
        .await
        .unwrap()
    );
    let mut forged = vec![RepositoryReplicaGap {
        source_node_id: cursor.source_node_id().into(),
        source_epoch: cursor.source_epoch(),
        stream: "traffic".into(),
        first_sequence: 1,
        last_sequence: 99,
        start_unix_seconds: NOW,
        end_unix_seconds: NOW,
        permanent: true,
        reason: Some("source_retention_expired".into()),
    }];
    assert!(super::super::ready_peer::protected::filter_gaps(&state, &mut forged).is_err());
    assert_eq!(forged.len(), 1);
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .replication_summary()
            .unwrap()
            .gaps
            .is_empty()
    );
}

#[tokio::test]
async fn expired_ready_tombstone_cursor_restarts_without_ack_or_duplicate_import() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    server.fault(peer::Fault::Pending).await;
    let running = state.clone();
    let task = tokio::spawn(async move {
        super::super::ready_peer::tombstones::before_local(&running, NOW).await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while server.requests().await.len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let storage = HistoryStorage::open(directory.path());
    let journal = serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap();
    server.fault(peer::Fault::Expired).await;
    assert_eq!(
        super::super::ready_peer::tombstones::before_local(&state, NOW + 5)
            .await
            .unwrap(),
        InitialBackfillProgress::Unavailable
    );
    assert_eq!(
        serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
        journal
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        super::super::ready_peer::tombstones::before_local(&state, NOW + 10)
            .await
            .unwrap(),
        InitialBackfillProgress::Complete
    );
    assert_eq!(storage.repository_history_record_count().unwrap(), 2);
    assert_eq!(
        serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
        journal
    );
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .replication_summary()
            .unwrap()
            .gaps
            .is_empty()
    );
}

#[tokio::test]
async fn ready_tombstones_precede_original_recovery_and_records_across_cancel_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let wires = stage(&state).await;
    let server = peer::Peer::start(&state, &wires).await;
    server.register(&state).await;
    membership(&state).await;
    server.fault(peer::Fault::Pending).await;
    let running_state = state.clone();
    let task = tokio::spawn(async move {
        super::super::super::advance_local_repository_lifecycle(&running_state, NOW).await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while server.requests().await.len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 2);
    assert!(
        server
            .requests()
            .await
            .iter()
            .all(|path| path.contains("initial-backfill"))
    );
    assert_eq!(storage.repository_history_record_count().unwrap(), 1);
    drop(state);
    let state = fixture::app_state(directory.path());
    for step in 1..20 {
        super::super::super::advance_local_repository_lifecycle(&state, NOW + step * 5)
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
    let journal = storage.source_delivery_journal().unwrap();
    assert_eq!(journal.len(), 1);
    assert_eq!(
        journal[0].wire, wires[2].wire,
        "only the original page received a real ACK"
    );
    let pending = SignedSegment::from_wire(&wires[1].wire).unwrap();
    let first = pending.canonical().first_cursor();
    assert_eq!(first.sequence(), 2);
    server
        .gap(RepositoryReplicaGap {
            source_node_id: first.source_node_id().into(),
            source_epoch: first.source_epoch(),
            stream: "traffic".into(),
            first_sequence: 0,
            last_sequence: 1,
            start_unix_seconds: NOW,
            end_unix_seconds: NOW,
            permanent: true,
            reason: Some("source_retention_expired".into()),
        })
        .await;
    drop(state);
    let state = fixture::app_state(directory.path());
    for step in 20..45 {
        super::super::super::advance_local_repository_lifecycle(&state, NOW + step * 5)
            .await
            .unwrap();
        let store = state.store.lock().await;
        let local = RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap();
        if store
            .state()
            .repository_membership
            .as_ref()
            .unwrap()
            .repository(&local)
            .unwrap()
            .catch_up_completed_at()
            .is_some()
        {
            break;
        }
    }
    let rows = storage
        .repository_history_records(None, None, None, 0, 128)
        .unwrap();
    let data = rows.iter().filter(|row| !row.tombstone).collect::<Vec<_>>();
    assert_eq!(data.len(), 4);
    assert!(data.iter().all(|row| row.record_key
        != format!("node-history:node:{}:old", state.cluster.node_id).as_bytes()));
    let runtime = state.repository_replica.lock().await;
    assert!(runtime.replication_summary().unwrap().gaps.is_empty());
    assert_eq!(runtime.local_source_next_sequence("traffic"), Some(5));
    assert!(
        runtime
            .initial_peer_backfill_checkpoint(PEER)
            .unwrap()
            .completed
    );
    assert!(
        runtime
            .initial_peer_backfill_checkpoint(PEER)
            .unwrap()
            .summary_complete
    );
}
