use super::*;
use crate::{
    history_sync::{CanonicalSegment, Cursor, SignedSegment, SyncRecord},
    http::history_repository::{derived_repository_identity, derived_repository_signing_key},
    state::{
        history_repository::{
            HistoryStorage,
            identity::RepositoryNodeId,
            replica::{RepositoryReplicaRuntime, RepositoryReplicaSegment},
        },
        history_storage::REPOSITORY_REPLICA_KEY,
    },
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

#[path = "app_state.rs"]
mod fixture;
#[path = "legacy_recovery_peer.rs"]
mod peer;

#[path = "legacy_watermark_peer.rs"]
mod watermark_peer;

const NOW: u64 = 1_800_000_000;

fn control(storage: &HistoryStorage) -> serde_json::Value {
    serde_json::from_slice(&storage.read(REPOSITORY_REPLICA_KEY).unwrap().unwrap()).unwrap()
}

fn stored_payload(row: &crate::state::history_storage::RepositoryHistoryRecordRow) -> Vec<u8> {
    let record: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    serde_json::from_value(record["payload"].clone()).unwrap()
}

fn inflight_contract(snapshot: &serde_json::Value) -> serde_json::Value {
    let mut page = snapshot["local_history_backfill_inflight"].clone();
    // A normal control-state write materializes serde defaults in predecessor checkpoints.
    // Compare the complete page contract, treating omitted defaults like their decoded values.
    if let Some(page) = page.as_object_mut() {
        for (key, value) in [
            ("segments", serde_json::json!([])),
            ("stream_state", serde_json::json!({})),
            ("epoch", serde_json::json!(0)),
            ("created_at_unix_seconds", serde_json::Value::Null),
        ] {
            page.entry(key).or_insert(value);
        }
    }
    page
}

fn sample(state: &AppState, sequence: u64) -> SyncRecord {
    // A real user_traffic-style key recurs. Distinct payloads/origins must all survive.
    SyncRecord::new(
        &state.cluster.node_id,
        &state.cluster.node_id,
        "traffic.v1",
        1,
        b"node-history:user:repeated:current".to_vec(),
        format!("sample-{sequence}").into_bytes(),
        false,
    )
}

async fn prepare(
    state: &AppState,
    storage: &HistoryStorage,
    head: u64,
) -> Vec<RepositoryReplicaSegment> {
    prepare_with_pending(
        state,
        storage,
        head,
        vec![sample(state, head), sample(state, head + 1)],
    )
    .await
}

async fn prepare_with_pending(
    state: &AppState,
    storage: &HistoryStorage,
    head: u64,
    pending_records: Vec<SyncRecord>,
) -> Vec<RepositoryReplicaSegment> {
    let pending_tail = pending_records
        .last()
        .expect("nonempty pending page")
        .record_key()
        .to_vec();
    let identity = derived_repository_identity(
        state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let key = derived_repository_signing_key(state, &state.cluster.node_id).unwrap();
    // Queue one real source record first so all source identity/epoch fields use runtime defaults.
    let seed = state
        .repository_replica
        .lock()
        .await
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity.clone(),
            &key,
            vec![sample(state, 0)],
            NOW - 2,
        )
        .unwrap()
        .unwrap();
    state
        .repository_replica
        .lock()
        .await
        .acknowledge_local_source_segment_via(&seed.wire, NOW - 1, "direct")
        .unwrap();
    let epoch = SignedSegment::from_wire(&seed.wire)
        .unwrap()
        .canonical()
        .first_cursor()
        .source_epoch();
    let mut previous = None;
    let mut prefix = Vec::new();
    for first in (0..head).step_by(128) {
        let last = (first + 128).min(head);
        let signed = CanonicalSegment::new(
            &state.cluster.cluster_id,
            Cursor::new(&state.cluster.node_id, epoch, "traffic", first).unwrap(),
            (first..last)
                .map(|sequence| sample(state, sequence))
                .collect(),
            previous,
            NOW - 2,
            NOW - 2,
        )
        .unwrap()
        .sign(&key)
        .unwrap();
        previous = Some(signed.segment_hash().unwrap());
        prefix.push(RepositoryReplicaSegment {
            identity: identity.clone(),
            wire: signed.wire_bytes().unwrap(),
        });
    }
    // Stage only the predecessor control boundary, with the actual signed prefix on the peer.
    let mut snapshot = control(storage);
    snapshot["local_source"]["streams"]["traffic"]["next_sequence"] = head.into();
    snapshot["local_source"]["streams"]["traffic"]["previous_segment_hash"] =
        serde_json::json!(previous);
    storage
        .write(
            REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    *state.repository_replica.lock().await =
        RepositoryReplicaRuntime::load(storage.clone()).unwrap();
    let pending = state
        .repository_replica
        .lock()
        .await
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity.clone(),
            &key,
            pending_records,
            NOW,
        )
        .unwrap()
        .unwrap();
    let signed = SignedSegment::from_wire(&pending.wire).unwrap();
    assert_eq!(signed.canonical().first_cursor().sequence(), head);
    assert_eq!(signed.canonical().previous_segment_hash(), previous);
    let mut snapshot = control(storage);
    let page_cursor = super::super::HistoricalBackfillSortKey {
        observed_at_unix_seconds: NOW,
        schema_id: "traffic.v1".into(),
        record_key: pending_tail,
    }
    .encode()
    .unwrap();
    snapshot["local_history_backfill_inflight"] = serde_json::json!({
        "page_cursor": page_cursor, "completed": false,
        "segment_ids": [hex::encode(Sha256::digest(&pending.wire))],
    });
    storage
        .write(
            REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    *state.repository_replica.lock().await =
        RepositoryReplicaRuntime::load(storage.clone()).unwrap();
    prefix
}

async fn tick(state: &AppState, now: u64) -> super::super::InitialBackfillProgress {
    super::super::backfill_initial_repository_from_local_history(state, now)
        .await
        .unwrap()
}

async fn drain(state: &AppState, start: u64) {
    for step in 0..128 {
        tick(state, start + step * RETRY_SECONDS).await;
        if state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_inflight_checkpoint()
            .is_none()
        {
            return;
        }
    }
    panic!("bounded worker ticks did not converge");
}

pub(crate) async fn assert_us4_recovery_and_next_page() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let storage = HistoryStorage::open(directory.path());
    let prefix = prepare(&state, &storage, 2262).await;
    // The receiver already owns an unrelated deletion; the local source has no tombstone stream.
    let other = "unrelated-other-source";
    let other_identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(other.to_owned()).unwrap(),
    )
    .unwrap();
    let other_key = derived_repository_signing_key(&state, other).unwrap();
    let deletion = CanonicalSegment::new(
        &state.cluster.cluster_id,
        Cursor::new(other, 7, "tombstone", 0).unwrap(),
        vec![SyncRecord::new(
            other,
            other,
            "traffic.v1",
            1,
            b"unrelated-deleted-key".to_vec(),
            Vec::new(),
            true,
        )],
        None,
        NOW - 3,
        NOW - 3,
    )
    .unwrap()
    .sign(&other_key)
    .unwrap();
    state
        .repository_replica
        .lock()
        .await
        .receive_wire_from_repository(
            &state.cluster.cluster_id,
            &other_identity,
            &deletion.wire_bytes().unwrap(),
            NOW,
            &[state.cluster.node_id.clone()],
            &state.cluster.node_id,
        )
        .unwrap();
    let mut snapshot = control(&storage);
    // us4's deletion ledger has a known recent verification, rather than an expired ledger.
    snapshot["last_verified_unix_seconds"] = NOW.into();
    storage
        .write(
            REPOSITORY_REPLICA_KEY,
            &serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    let live = snapshot["local_source"].clone();
    let tombstones = snapshot["tombstones"].clone();
    assert!(live["streams"].get("tombstone").is_none());
    let journal = storage.source_delivery_journal().unwrap();
    assert_eq!(journal.len(), 1);
    let peer = peer::Peer::start(&state, &prefix).await;
    peer.register(&state).await;
    drop(state);
    let state = fixture::app_state(directory.path());
    peer.register(&state).await;
    // Every bounded page/chain step is durable before the next request or a restart.
    assert_eq!(
        tick(&state, NOW).await,
        super::super::InitialBackfillProgress::InProgress
    );
    assert_eq!(storage.repository_history_record_count().unwrap(), 1);
    assert_eq!(
        storage.source_delivery_journal().unwrap()[0].wire,
        journal[0].wire
    );
    let scan: serde_json::Value =
        serde_json::from_slice(&storage.legacy_recovery_checkpoint().unwrap().unwrap()).unwrap();
    assert!(scan["targets"]["traffic"]["next"].as_u64().unwrap() > 0);
    let scan_requests = peer
        .requests()
        .await
        .into_iter()
        .filter(|path| path.contains("/summary?") || path.ends_with("/repair"))
        .count();
    assert_eq!(
        scan_requests, MAX_STEPS_PER_TICK,
        "one tick makes bounded batch progress"
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    peer.register(&state).await;
    peer.fault(peer::Fault::Truncated).await;
    assert_eq!(
        tick(&state, NOW).await,
        super::super::InitialBackfillProgress::Unavailable
    );
    assert_eq!(control(&storage)["local_source"], live);
    assert_eq!(
        inflight_contract(&control(&storage)),
        inflight_contract(&snapshot)
    );
    assert_eq!(storage.repository_history_record_count().unwrap(), 1);
    assert_eq!(
        storage.source_delivery_journal().unwrap()[0].wire,
        journal[0].wire
    );
    drain(&state, NOW + RETRY_SECONDS).await;
    // Older source checkpoints can omit node_id. Recovery binds it to the verified signing
    // identity without altering the epoch, stream counters, hashes, or pending source state.
    let mut expected_live = live.clone();
    if expected_live["node_id"].is_null() {
        expected_live["node_id"] = state.cluster.node_id.clone().into();
    }
    assert_eq!(control(&storage)["local_source"], expected_live);
    assert_eq!(control(&storage)["tombstones"], tombstones);
    assert!(
        storage.source_delivery_journal().unwrap().is_empty(),
        "real durable ACK"
    );
    assert!(control(&storage)["gaps"].as_array().unwrap().is_empty());
    assert_eq!(storage.repository_history_record_count().unwrap(), 2265);
    assert_eq!(
        storage.repository_history_segment_count().unwrap(),
        prefix.len() + 2
    );
    let original_ids = prefix
        .iter()
        .map(|row| hex::encode(Sha256::digest(&row.wire)))
        .chain(journal.iter().map(|row| row.id.clone()))
        .collect::<Vec<_>>();
    // Repair responses are wire-byte bounded, so inspect the original wires in small pages.
    for ids in original_ids.chunks(4) {
        let stored = state
            .repository_replica
            .lock()
            .await
            .repair_batch(ids)
            .unwrap()
            .segments;
        assert_eq!(stored.len(), ids.len());
        for row in &stored {
            let signed = SignedSegment::from_wire(&row.wire).unwrap();
            signed.verify_identity(&row.identity).unwrap();
            assert!(ids.contains(&hex::encode(Sha256::digest(&row.wire))));
            assert_eq!(signed.canonical().first_cursor().stream(), "traffic");
        }
    }
    let requests = peer.requests().await;
    assert!(
        requests
            .iter()
            .any(|path| path.contains("after_segment_id="))
    );
    assert!(
        requests
            .iter()
            .filter(|path| path.ends_with("/repair"))
            .count()
            > 1
    );
    let rows = storage
        .repository_history_records(None, None, None, 0, 3000)
        .unwrap();
    let samples = rows
        .iter()
        .filter(|row| row.source_node_id == state.cluster.node_id)
        .map(|row| (row.sequence, stored_payload(row)))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(samples.len(), 2264);
    for sequence in 0..2264 {
        assert_eq!(samples[&sequence], format!("sample-{sequence}").as_bytes());
    }
    drop(state);
    let state = fixture::app_state(directory.path());
    peer.register(&state).await;
    assert_eq!(
        recover_legacy_original_page(
            &state,
            &journal[0].identity,
            NOW + 1000,
            &[state.cluster.node_id.clone()],
        )
        .await
        .unwrap(),
        LegacyRecoveryOutcome::NotLegacy
    );
    // A later new page legitimately uses the same recurring key and a distinct payload.
    // Queue it through the normal durable API, then apply it through the actual worker.
    let identity = journal[0].identity.clone();
    let key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
    let new_live = state
        .repository_replica
        .lock()
        .await
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity.clone(),
            &key,
            vec![sample(&state, 2264)],
            NOW + 1000,
        )
        .unwrap()
        .unwrap();
    let signed_live = SignedSegment::from_wire(&new_live.wire).unwrap();
    assert_eq!(signed_live.canonical().first_cursor().sequence(), 2264);
    assert_eq!(
        signed_live.canonical().previous_segment_hash(),
        Some(
            SignedSegment::from_wire(&journal[0].wire)
                .unwrap()
                .segment_hash()
                .unwrap(),
        )
    );
    let live = control(&storage)["local_source"].clone();
    let created = state
        .repository_replica
        .lock()
        .await
        .queue_local_history_backfill_batches_at(
            &state.cluster.cluster_id,
            identity,
            &key,
            None,
            true,
            vec![(vec![sample(&state, 9999), sample(&state, 0)], NOW + 1)],
            NOW + 1000,
        )
        .unwrap();
    assert_eq!(created.len(), 1);
    let next = SignedSegment::from_wire(&created[0].wire).unwrap();
    assert_eq!(next.canonical().first_cursor().sequence(), 0);
    assert!(
        next.canonical()
            .first_cursor()
            .stream()
            .contains("-backfill-")
    );
    assert_eq!(
        tick(&state, NOW + 1000).await,
        super::super::InitialBackfillProgress::Complete
    );
    assert_eq!(storage.repository_history_record_count().unwrap(), 2267);
    assert_eq!(control(&storage)["local_source"], live);
    drop(state);
    let state = fixture::app_state(directory.path());
    for _ in 0..2 {
        assert_eq!(
            tick(&state, NOW + 1000).await,
            super::super::InitialBackfillProgress::Complete
        );
    }
    assert_eq!(storage.repository_history_record_count().unwrap(), 2267);
    let pending = storage.source_delivery_journal().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].wire, new_live.wire);
    let rows = storage
        .repository_history_records(None, None, None, 0, 3000)
        .unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| stored_payload(row) == b"sample-9999")
            .count(),
        1
    );
    assert_eq!(control(&storage)["local_source"], live);
    assert_eq!(
        rows.iter()
            .filter(|row| stored_payload(row) == b"sample-0")
            .count(),
        2
    );
}

#[tokio::test]
async fn actual_worker_preserves_original_state_on_bad_public_peer_responses() {
    for fault in [
        peer::Fault::Missing,
        peer::Fault::InvalidAck,
        peer::Fault::Cursor,
        peer::Fault::Oversized,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let state = fixture::app_state(directory.path());
        let storage = HistoryStorage::open(directory.path());
        let prefix = prepare(&state, &storage, 4).await;
        let peer = peer::Peer::start(&state, &prefix).await;
        peer.register(&state).await;
        let before = control(&storage);
        let journal = serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap();
        peer.fault(fault).await;
        for step in 0..2 {
            tick(&state, NOW + step).await;
        }
        assert_eq!(
            storage.repository_history_record_count().unwrap(),
            0,
            "{fault:?}"
        );
        for field in ["local_source", "receiver", "gaps"] {
            assert_eq!(
                control(&storage)[field],
                before[field],
                "{fault:?}: {field}"
            );
        }
        assert_eq!(
            inflight_contract(&control(&storage)),
            inflight_contract(&before)
        );
        assert_eq!(
            serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
            journal
        );
        drop(state);
        let state = fixture::app_state(directory.path());
        peer.register(&state).await;
        assert_eq!(
            inflight_contract(&control(&storage)),
            inflight_contract(&before)
        );
        assert_eq!(
            serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
            journal
        );
        assert!(control(&storage)["gaps"].as_array().unwrap().is_empty());
        eprintln!("validated signed public peer fault {fault:?} across restart");
    }
}

#[tokio::test]
async fn actual_worker_rejects_signed_peer_invalid_source_evidence_without_ack() {
    for fault in ["signature", "identity", "chain"] {
        let directory = tempfile::tempdir().unwrap();
        let state = fixture::app_state(directory.path());
        let storage = HistoryStorage::open(directory.path());
        let prefix = prepare(&state, &storage, 4).await;
        let peer = peer::Peer::start(&state, &prefix).await;
        peer.register(&state).await;
        let mut invalid = prefix[0].clone();
        let signed = SignedSegment::from_wire(&invalid.wire).unwrap();
        if fault == "identity" {
            invalid.identity = derived_repository_identity(
                &state,
                RepositoryNodeId::try_from("unrelated-identity".to_owned()).unwrap(),
            )
            .unwrap();
        } else if fault == "signature" {
            invalid.wire = signed
                .canonical()
                .sign(&ed25519_dalek::SigningKey::from_bytes(&[79; 32]))
                .unwrap()
                .wire_bytes()
                .unwrap();
        } else {
            let key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
            invalid.wire = CanonicalSegment::new(
                &state.cluster.cluster_id,
                signed.canonical().first_cursor().clone(),
                signed.canonical().records().to_vec(),
                Some([42; 32]),
                NOW - 2,
                NOW - 2,
            )
            .unwrap()
            .sign(&key)
            .unwrap()
            .wire_bytes()
            .unwrap();
            SignedSegment::from_wire(&invalid.wire)
                .unwrap()
                .verify_identity(&invalid.identity)
                .unwrap();
        }
        peer.evidence(Some(invalid)).await;
        let before = control(&storage);
        let journal = serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap();
        tick(&state, NOW).await;
        tick(&state, NOW).await;
        assert_eq!(
            storage.repository_history_record_count().unwrap(),
            0,
            "{fault}"
        );
        for field in ["local_source", "receiver", "gaps"] {
            assert_eq!(control(&storage)[field], before[field], "{fault}: {field}");
        }
        assert_eq!(
            inflight_contract(&control(&storage)),
            inflight_contract(&before)
        );
        assert_eq!(
            serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
            journal
        );
        peer.evidence(None).await;
        drop(state);
        let state = fixture::app_state(directory.path());
        peer.register(&state).await;
        assert_eq!(
            inflight_contract(&control(&storage)),
            inflight_contract(&before)
        );
        assert_eq!(
            serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
            journal
        );
        eprintln!("validated signed peer/source {fault} rejection across restart");
    }
}

#[tokio::test]
async fn actual_worker_bounds_slow_peer_and_cancellation_preserves_repair_checkpoint() {
    for cancel in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let state = std::sync::Arc::new(fixture::app_state(directory.path()));
        let storage = HistoryStorage::open(directory.path());
        let prefix = prepare(&state, &storage, 4).await;
        let peer = peer::Peer::start(&state, &prefix).await;
        peer.register(&state).await;
        peer.fault(peer::Fault::Pending).await;
        let before = control(&storage);
        let journal = serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap();
        let worker_state = state.clone();
        let identity = storage.source_delivery_journal().unwrap()[0]
            .identity
            .clone();
        let started = tokio::time::Instant::now();
        let task = tokio::spawn(async move {
            recover_legacy_original_page(
                &worker_state,
                &identity,
                NOW,
                &[worker_state.cluster.node_id.clone()],
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            while !peer
                .requests()
                .await
                .iter()
                .any(|path| path.ends_with("/repair"))
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker reaches signed repair body without sleeping");
        if cancel {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            tokio::time::pause();
            tokio::time::advance(REQUEST_BUDGET).await;
            assert_eq!(
                task.await.unwrap().unwrap(),
                LegacyRecoveryOutcome::Unavailable
            );
            assert!(started.elapsed() < TICK_BUDGET);
            tokio::time::resume();
        }
        assert_eq!(storage.repository_history_record_count().unwrap(), 0);
        assert_eq!(control(&storage)["local_source"], before["local_source"]);
        assert_eq!(
            inflight_contract(&control(&storage)),
            inflight_contract(&before)
        );
        assert_eq!(
            serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
            journal
        );
        let scan: serde_json::Value =
            serde_json::from_slice(&storage.legacy_recovery_checkpoint().unwrap().unwrap())
                .unwrap();
        assert_eq!(
            scan["pending"].as_array().unwrap().len(),
            usize::from(cancel)
        );
        drop(state);
        let state = fixture::app_state(directory.path());
        peer.register(&state).await;
        let identity = storage.source_delivery_journal().unwrap()[0]
            .identity
            .clone();
        if !cancel {
            assert_eq!(
                recover_legacy_original_page_step(
                    &state,
                    &identity,
                    NOW + RETRY_SECONDS,
                    &[state.cluster.node_id.clone()],
                )
                .await
                .unwrap(),
                LegacyRecoveryOutcome::InProgress
            );
        }
        let requests_before = peer.requests().await.len();
        assert_eq!(
            recover_legacy_original_page_step(
                &state,
                &identity,
                NOW + RETRY_SECONDS,
                &[state.cluster.node_id.clone()],
            )
            .await
            .unwrap(),
            LegacyRecoveryOutcome::InProgress
        );
        let requests = peer.requests().await;
        assert_eq!(requests.len(), requests_before + 1);
        assert!(
            requests.last().unwrap().ends_with("/repair"),
            "retry exact durable repair"
        );
        let scan: serde_json::Value =
            serde_json::from_slice(&storage.legacy_recovery_checkpoint().unwrap().unwrap())
                .unwrap();
        assert_eq!(scan["phase"], "apply");
        assert_eq!(scan["targets"]["traffic"]["next"], 4);
        assert_eq!(storage.repository_history_record_count().unwrap(), 0);
        assert_eq!(
            serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
            journal
        );
    }
}

#[tokio::test]
async fn actual_worker_accepts_new_independent_page_after_original_stream_without_key_dedup() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let storage = HistoryStorage::open(directory.path());
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
    let source = state
        .repository_replica
        .lock()
        .await
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity.clone(),
            &key,
            vec![sample(&state, 0)],
            NOW,
        )
        .unwrap()
        .unwrap();
    let receipt = state
        .repository_replica
        .lock()
        .await
        .receive_wire_from_repository(
            &state.cluster.cluster_id,
            &identity,
            &source.wire,
            NOW,
            &[state.cluster.node_id.clone()],
            &state.cluster.node_id,
        )
        .unwrap();
    let receipt = serde_json::to_value(receipt).unwrap();
    assert_eq!(receipt["acknowledgement"]["sequence"], 0);
    assert!(receipt.get("gap").is_none());
    state
        .repository_replica
        .lock()
        .await
        .acknowledge_local_source_segment_via(&source.wire, NOW, "direct")
        .unwrap();
    let live = control(&storage)["local_source"].clone();
    state
        .repository_replica
        .lock()
        .await
        .queue_local_history_backfill_batches_at(
            &state.cluster.cluster_id,
            identity,
            &key,
            None,
            true,
            vec![(vec![sample(&state, 0), sample(&state, 1)], NOW + 1)],
            NOW + 1,
        )
        .unwrap();
    assert_eq!(
        tick(&state, NOW + 1).await,
        super::super::InitialBackfillProgress::Complete
    );
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        tick(&state, NOW + 1).await,
        super::super::InitialBackfillProgress::Complete
    );
    assert_eq!(storage.repository_history_record_count().unwrap(), 3);
    let rows = storage
        .repository_history_records(None, None, None, 0, 4)
        .unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| stored_payload(row) == b"sample-0")
            .count(),
        2
    );
    assert_eq!(
        rows.iter()
            .filter(|row| stored_payload(row) == b"sample-1")
            .count(),
        1
    );
    assert_eq!(control(&storage)["local_source"], live);
    assert!(storage.source_delivery_journal().unwrap().is_empty());
}

#[tokio::test]
async fn actual_worker_makes_eight_bounded_network_steps_in_one_tick() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let storage = HistoryStorage::open(directory.path());
    let prefix = prepare(&state, &storage, 2262).await;
    let peer = peer::Peer::start(&state, &prefix).await;
    peer.register(&state).await;
    let before = control(&storage);
    assert_eq!(
        tick(&state, NOW).await,
        super::super::InitialBackfillProgress::InProgress
    );
    let requests = peer
        .requests()
        .await
        .into_iter()
        .filter(|path| path.contains("/summary?") || path.ends_with("/repair"))
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), MAX_STEPS_PER_TICK);
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.ends_with("/repair"))
            .count(),
        4
    );
    let scan: serde_json::Value =
        serde_json::from_slice(&storage.legacy_recovery_checkpoint().unwrap().unwrap()).unwrap();
    assert_eq!(scan["phase"], "scan");
    assert_eq!(scan["targets"]["traffic"]["next"], 1536);
    assert_eq!(storage.legacy_recovery_staged_after(0).unwrap().len(), 12);
    assert_eq!(storage.repository_history_record_count().unwrap(), 0);
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 1);
    assert_eq!(control(&storage)["local_source"], before["local_source"]);
    assert_eq!(
        inflight_contract(&control(&storage)),
        inflight_contract(&before)
    );
}

#[tokio::test]
async fn worker_recovers_full_us4_style_node_page_after_nonzero_live_sequence() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let storage = HistoryStorage::open(directory.path());
    let node = state.cluster.node_id.clone();
    let pending = (0..128)
        .map(|index| {
            SyncRecord::new(
                &node,
                &node,
                "traffic.v1",
                1,
                format!("node-history:node:{node}:minute:{index:03}").into_bytes(),
                format!("node-sample-{index}").into_bytes(),
                false,
            )
        })
        .collect();
    let prefix = prepare_with_pending(&state, &storage, 2262, pending).await;
    let original = storage.source_delivery_journal().unwrap();
    assert_eq!(original.len(), 1);
    let signed = SignedSegment::from_wire(&original[0].wire).unwrap();
    assert_eq!(signed.canonical().first_cursor().sequence(), 2262);
    assert_eq!(signed.canonical().records().len(), 128);
    let peer = peer::Peer::start(&state, &prefix).await;
    peer.register(&state).await;
    drain(&state, NOW).await;
    assert!(storage.source_delivery_journal().unwrap().is_empty());
    drop(state);
    let state = fixture::app_state(directory.path());
    assert!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    let rows = storage
        .repository_history_records(None, None, None, 0, 3000)
        .unwrap();
    assert_eq!(rows.len(), 2390);
    assert_eq!(
        rows.iter()
            .filter(|row| row
                .record_key
                .starts_with(format!("node-history:node:{node}:").as_bytes()))
            .count(),
        128
    );
    let stored = state
        .repository_replica
        .lock()
        .await
        .repair_batch(&[original[0].id.clone()])
        .unwrap();
    assert_eq!(stored.segments.len(), 1);
    assert_eq!(stored.segments[0].wire, original[0].wire);
}

#[tokio::test]
async fn first_repository_imports_peer_deletions_before_legacy_page() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let storage = HistoryStorage::open(directory.path());
    let prefix = prepare(&state, &storage, 8).await;
    let original = storage.source_delivery_journal().unwrap();
    let peer = peer::Peer::start(&state, &prefix).await;
    peer.initial_page(serde_json::json!({
        "records": [{
            "observed_at_unix_seconds": NOW - 3,
            "subject_node_id": &state.cluster.node_id,
            "observer_node_id": &state.cluster.node_id,
            "schema_id": "traffic.v1", "schema_version": 1,
            "record_key_base64": URL_SAFE_NO_PAD.encode(b"node-history:user:repeated:"),
            "payload_base64": URL_SAFE_NO_PAD.encode(b""),
            "tombstone": true
        }]
    }))
    .await;
    peer.register(&state).await;
    drain(&state, NOW).await;
    assert!(storage.source_delivery_journal().unwrap().is_empty());
    assert!(
        storage
            .repository_history_records(None, None, None, 0, 50)
            .unwrap()
            .is_empty()
    );
    let requests = peer.requests().await;
    let initial = requests
        .iter()
        .position(|path| path.contains("/initial-backfill"))
        .unwrap();
    let summary = requests
        .iter()
        .position(|path| path.contains("/summary?"))
        .unwrap();
    assert!(initial < summary);
    let stored = state
        .repository_replica
        .lock()
        .await
        .repair_batch(&[original[0].id.clone()])
        .unwrap();
    assert_eq!(stored.segments[0].wire, original[0].wire);
}
