use super::super::*;
use crate::history_sync::SignedSegment;
use crate::node_history::{NodeHistorySnapshot, NodeTrafficBucket, NodeTrafficRollupSnapshot};
use crate::state::history_repository::{HistoryStorage, replica::RepositoryReplicaRuntime};

#[path = "app_state.rs"]
mod fixture;

#[tokio::test]
async fn first_repository_worker_resumes_local_history_without_consuming_advanced_live_wire() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let now = chrono::Utc::now();
    let now_seconds = u64::try_from(now.timestamp()).unwrap();
    let record_count = MAX_INITIAL_BACKFILL_PAGE_RECORDS * 2 + 1;
    let buckets: Vec<_> = (0..record_count)
        .map(|index| {
            let end = now - chrono::Duration::minutes(((record_count - index) * 5) as i64);
            NodeTrafficBucket {
                start_at: (end - chrono::Duration::minutes(5)).to_rfc3339(),
                end_at: end.to_rfc3339(),
                uplink_bytes: Some(index as u64),
                downlink_bytes: Some(2 * index as u64),
                complete: true,
                warnings: Vec::new(),
            }
        })
        .collect();
    let expected = buckets
        .iter()
        .map(|bucket| {
            (
                format!(
                    "node-history:node:{}:five-minute:{}",
                    state.cluster.node_id, bucket.start_at
                )
                .into_bytes(),
                (
                    u64::try_from(
                        chrono::DateTime::parse_from_rfc3339(&bucket.end_at)
                            .unwrap()
                            .timestamp(),
                    )
                    .unwrap(),
                    serde_json::to_value(bucket).unwrap(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    state
        .node_history
        .replace_node_snapshot(
            now,
            &state.cluster.node_id,
            NodeHistorySnapshot {
                node_id: state.cluster.node_id.clone(),
                last_synced_at: None,
                last_sync_error: None,
                daily_traffic: Vec::new(),
                daily_component_status: Vec::new(),
                component_status_events: Vec::new(),
                traffic: Some(NodeTrafficRollupSnapshot {
                    five_minute: buckets,
                    ..Default::default()
                }),
                user_traffic_users: Vec::new(),
            },
        )
        .await;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
    )
    .unwrap();
    let signing_key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
    let storage = HistoryStorage::open(directory.path());
    // The node has already delivered history before it becomes the first repository. Its live
    // traffic chain therefore starts beyond zero, and still has an unacknowledged signed wire.
    let mut prior_sequence = None;
    let mut pending_wire = Vec::new();
    for index in 0..4 {
        let mut runtime = state.repository_replica.lock().await;
        let segment = runtime
            .queue_local_source_segment(
                &state.cluster.cluster_id,
                identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    &state.cluster.node_id,
                    &state.cluster.node_id,
                    "traffic.v1",
                    1,
                    format!("live-{index}").into_bytes(),
                    b"live".to_vec(),
                    false,
                )],
                now_seconds,
            )
            .unwrap()
            .unwrap();
        let signed = SignedSegment::from_wire(&segment.wire).unwrap();
        let sequence = signed.canonical().first_cursor().sequence();
        if let Some(previous) = prior_sequence {
            assert_eq!(sequence, previous + 1);
        }
        prior_sequence = Some(sequence);
        if index < 3 {
            runtime
                .acknowledge_local_source_segment_via(&segment.wire, now_seconds, "direct")
                .unwrap();
        } else {
            assert!(sequence > 0);
            pending_wire = segment.wire;
        }
    }
    let journal_before = storage.source_delivery_journal_summary().unwrap();
    assert_eq!(journal_before.pending_segments, 1);
    let live_checkpoint = || {
        let snapshot: serde_json::Value = serde_json::from_slice(
            &storage
                .read(crate::state::history_storage::REPOSITORY_REPLICA_KEY)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        snapshot
            .get("local_source")
            .expect("durable live source checkpoint")
            .clone()
    };
    let live_before = live_checkpoint();
    assert_eq!(storage.repository_history_record_count().unwrap(), 0);
    assert_eq!(
        backfill_initial_repository_from_local_history(&state, now_seconds)
            .await
            .unwrap(),
        InitialBackfillProgress::InProgress
    );
    let cursor = state
        .repository_replica
        .lock()
        .await
        .local_history_backfill_cursor()
        .unwrap()
        .to_owned();
    assert_eq!(
        storage.repository_history_record_count().unwrap(),
        MAX_INITIAL_BACKFILL_PAGE_RECORDS
    );

    // Persist the actual second collector page at the same durable boundary used by the worker.
    // The existing write-failure hook acts after queueing; it cannot interrupt an opaque tick
    // between queue and apply, so only that boundary is staged explicitly here.
    let collected = historical_source_backfill_records(
        &state,
        Some(&cursor),
        MAX_INITIAL_BACKFILL_PAGE_RECORDS,
    )
    .await
    .unwrap();
    let next_cursor = collected.next_cursor().unwrap().expect("third page");
    let batches = queued_history_backfill_batches(
        historical_record_batches(collected.into_records()).unwrap(),
    );
    let staged = state
        .repository_replica
        .lock()
        .await
        .queue_local_history_backfill_batches_at(
            &state.cluster.cluster_id,
            identity.clone(),
            &signing_key,
            Some(next_cursor.clone()),
            false,
            batches,
            now_seconds,
        )
        .unwrap();
    assert!(!staged.is_empty());
    // Use the runtime's storage connection, shared by HistoryStorage::open in this process.
    storage.set_query_only_for_test(true).unwrap();
    let interrupted = backfill_initial_repository_from_local_history(&state, now_seconds).await;
    storage.set_query_only_for_test(false).unwrap();
    assert!(
        interrupted.is_err(),
        "the worker must surface a failed page application"
    );
    assert_eq!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_cursor(),
        Some(cursor.as_str())
    );
    assert_eq!(
        storage.repository_history_record_count().unwrap(),
        MAX_INITIAL_BACKFILL_PAGE_RECORDS
    );
    drop(state);

    let state = fixture::app_state(directory.path());
    {
        let runtime = state.repository_replica.lock().await;
        assert_eq!(
            runtime.local_history_backfill_cursor(),
            Some(cursor.as_str())
        );
        assert_eq!(
            runtime.local_history_backfill_inflight_checkpoint(),
            Some((Some(next_cursor.clone()), false))
        );
        let restored = runtime.local_history_backfill_inflight_segments().unwrap();
        assert_eq!(
            restored.iter().map(|s| &s.wire).collect::<Vec<_>>(),
            staged.iter().map(|s| &s.wire).collect::<Vec<_>>()
        );
        assert_eq!(
            runtime.local_source_pending_segments()[0].wire,
            pending_wire
        );
    }
    assert_eq!(
        backfill_initial_repository_from_local_history(&state, now_seconds)
            .await
            .unwrap(),
        InitialBackfillProgress::InProgress
    );
    assert_eq!(
        state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_cursor(),
        Some(next_cursor.as_str())
    );
    assert_eq!(
        storage.repository_history_record_count().unwrap(),
        MAX_INITIAL_BACKFILL_PAGE_RECORDS * 2
    );
    assert_eq!(
        backfill_initial_repository_from_local_history(&state, now_seconds)
            .await
            .unwrap(),
        InitialBackfillProgress::Complete
    );
    // Completion is durable and subsequent ticks are idempotent.
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        backfill_initial_repository_from_local_history(&state, now_seconds)
            .await
            .unwrap(),
        InitialBackfillProgress::Complete
    );
    let runtime = state.repository_replica.lock().await;
    assert!(runtime.local_history_backfill_completed());
    assert!(
        runtime
            .local_history_backfill_inflight_checkpoint()
            .is_none()
    );
    assert_eq!(runtime.local_source_pending_segments().len(), 1);
    assert_eq!(
        runtime.local_source_pending_segments()[0].wire,
        pending_wire
    );
    let journal_after = storage.source_delivery_journal_summary().unwrap();
    assert_eq!(
        journal_after.pending_segments,
        journal_before.pending_segments
    );
    assert_eq!(
        journal_after.last_acknowledged_at,
        journal_before.last_acknowledged_at
    );
    let rows = storage
        .repository_history_records(None, None, None, 0, record_count + 1)
        .unwrap();
    assert_eq!(rows.len(), record_count);
    assert!(
        rows.iter()
            .all(|r| r.stream == format!("traffic-backfill-{}", state.cluster.node_id))
    );
    let actual = rows
        .iter()
        .map(|row| {
            // SQLite stores the serialized StoredRecord envelope around the source payload.
            let stored: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
            let payload: Vec<u8> = serde_json::from_value(stored["payload"].clone()).unwrap();
            (
                row.record_key.clone(),
                (
                    row.observed_end_unix_seconds,
                    serde_json::from_slice::<serde_json::Value>(&payload).unwrap(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(actual.len(), expected.len());
    for (key, expected_record) in expected {
        assert_eq!(
            actual.get(&key),
            Some(&expected_record),
            "{}",
            String::from_utf8_lossy(&key)
        );
    }
    assert_eq!(live_checkpoint(), live_before);
    assert_eq!(journal_after.pending_bytes, journal_before.pending_bytes);
    assert_eq!(
        journal_after.last_delivery_path,
        journal_before.last_delivery_path
    );
    // Backfill must leave the live generator checkpoint intact, not merely retain its wire.
    drop(runtime);
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).unwrap();
    runtime
        .queue_local_source_segment(
            &state.cluster.cluster_id,
            identity,
            &signing_key,
            vec![SyncRecord::new(
                &state.cluster.node_id,
                &state.cluster.node_id,
                "traffic.v1",
                1,
                b"live-next".to_vec(),
                b"next".to_vec(),
                false,
            )],
            now_seconds,
        )
        .unwrap()
        .unwrap();
    let queued = runtime.local_source_pending_segments_page_with_budget(2, 1024 * 1024);
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].wire, pending_wire);
    let next = &queued[1];
    let pending_signed = SignedSegment::from_wire(&pending_wire).unwrap();
    let next_signed = SignedSegment::from_wire(&next.wire).unwrap();
    assert_eq!(
        next_signed.canonical().first_cursor().source_epoch(),
        pending_signed.canonical().first_cursor().source_epoch()
    );
    assert_eq!(
        next_signed.canonical().previous_segment_hash(),
        Some(pending_signed.segment_hash().unwrap())
    );
    assert_eq!(
        SignedSegment::from_wire(&next.wire)
            .unwrap()
            .canonical()
            .first_cursor()
            .sequence(),
        prior_sequence.unwrap() + 1
    );
}
