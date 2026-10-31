use crate::history_sync::SyncRecord;
#[cfg(test)]
use crate::state::history_repository::control::RepositoryLifecycle;
use crate::state::history_repository::replica::{RepositoryReplicaGap, RepositoryReplicaSegment};
use std::{future::Future, time::Duration};

use super::super::AppState;
pub(super) const MAX_SOURCE_PAYLOAD_BYTES: usize = 32 * 1024;

pub(super) async fn run_local_source_worker_cycle<Capacity, Lifecycle, Source>(
    capacity: Capacity,
    lifecycle: Lifecycle,
    source: Source,
) -> (anyhow::Result<()>, anyhow::Result<()>, anyhow::Result<()>)
where
    Capacity: std::future::Future<Output = anyhow::Result<()>>,
    Lifecycle: std::future::Future<Output = anyhow::Result<()>>,
    Source: std::future::Future<Output = anyhow::Result<()>>,
{
    run_local_source_worker_cycle_with_budget(
        capacity,
        lifecycle,
        source,
        super::REPOSITORY_REQUEST_BUDGET,
    )
    .await
}

async fn run_local_source_worker_cycle_with_budget<Capacity, Lifecycle, Source>(
    capacity: Capacity,
    lifecycle: Lifecycle,
    source: Source,
    maintenance_budget: Duration,
) -> (anyhow::Result<()>, anyhow::Result<()>, anyhow::Result<()>)
where
    Capacity: Future<Output = anyhow::Result<()>>,
    Lifecycle: Future<Output = anyhow::Result<()>>,
    Source: Future<Output = anyhow::Result<()>>,
{
    let capacity = bounded_maintenance("capacity", capacity, maintenance_budget);
    let lifecycle = bounded_maintenance("lifecycle", lifecycle, maintenance_budget);
    tokio::join!(capacity, lifecycle, source)
}

async fn bounded_maintenance<F>(
    name: &'static str,
    future: F,
    budget: Duration,
) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    tokio::time::timeout(budget, future)
        .await
        .map_err(|_| anyhow::anyhow!("history repository {name} maintenance timed out"))?
}

pub(super) fn spawn_local_source_worker(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(super::SOURCE_COLLECTION_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
            let mut runtime = state.repository_replica.lock().await;
            let repair_result = super::repository_blocking(|| {
                runtime.repair_source_delivery_journal_order_pages(
                    crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_REPAIR_PAGES_PER_CYCLE,
                )
            });
            drop(runtime);
            if let Err(error) = repair_result {
                tracing::debug!(
                    error = %error,
                    "history source journal order repair cycle skipped"
                );
            }
            let (capacity_result, lifecycle_result, source_result) = run_local_source_worker_cycle(
                super::sync_local_repository_capacity(&state),
                super::advance_local_repository_lifecycle(&state, now),
                super::source_records::publish_local_history_segments(&state),
            )
            .await;
            if let Err(error) = capacity_result {
                tracing::debug!(error = %error, "history repository capacity cycle skipped");
            }
            if let Err(error) = lifecycle_result {
                tracing::debug!(error = %error, "history repository lifecycle cycle skipped");
            }
            if let Err(error) = source_result {
                tracing::debug!(error = %error, "history source collection cycle skipped");
            }
        }
    });
}

pub(super) async fn local_history_backfill_inflight(state: &AppState) -> bool {
    state
        .repository_replica
        .lock()
        .await
        .local_history_backfill_inflight_checkpoint()
        .is_some()
}

pub(super) async fn repair_legacy_tombstone_metadata(
    state: &AppState,
    now_unix_seconds: u64,
) -> anyhow::Result<()> {
    let mut runtime = state.repository_replica.lock().await;
    super::repository_blocking(|| runtime.repair_legacy_tombstone_metadata(now_unix_seconds))?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn should_fanout_tombstone_acknowledgements(lifecycle: RepositoryLifecycle) -> bool {
    lifecycle == RepositoryLifecycle::Ready
}

pub(super) async fn receive_local_source_segments(
    state: &AppState,
    segments: &[RepositoryReplicaSegment],
    gaps: &[RepositoryReplicaGap],
    ready_repository_ids: &[String],
    now: u64,
) -> anyhow::Result<Vec<crate::state::history_repository::replica::RepositorySyncReceipt>> {
    // Keep local apply bounded while avoiding the old four-segment/500ms cadence, which capped
    // repository apply below the source's required ten-times headroom during backlog recovery.
    const MAX_LOCAL_BATCH_SEGMENTS: usize = 4;
    const LOCAL_BATCH_YIELD: Duration = Duration::from_millis(50);
    let batch_count = segments.len().div_ceil(MAX_LOCAL_BATCH_SEGMENTS);
    let mut receipts = Vec::with_capacity(segments.len());
    for (batch_index, batch) in segments.chunks(MAX_LOCAL_BATCH_SEGMENTS).enumerate() {
        let batch_gaps = if batch_index == 0 { gaps } else { &[] };
        let mut runtime = state.repository_replica.lock().await;
        let batch_receipts = super::repository_blocking(|| {
            runtime.receive_wire_batch_from_repository_with_gaps(
                &state.cluster.cluster_id,
                batch,
                batch_gaps,
                now,
                ready_repository_ids,
                &state.cluster.node_id,
            )
        })?;
        drop(runtime);
        receipts.extend(batch_receipts);
        if batch_index + 1 < batch_count {
            tokio::time::sleep(LOCAL_BATCH_YIELD).await;
        }
    }
    Ok(receipts)
}

pub(super) fn source_record(
    schema_id: &str,
    node_id: &str,
    now: u64,
    payload: serde_json::Value,
    tombstone: bool,
) -> anyhow::Result<SyncRecord> {
    source_record_with_key(
        schema_id,
        node_id,
        now,
        format!("node-history:node:{node_id}:current:{schema_id}:{now}").into_bytes(),
        payload,
        tombstone,
    )
}

pub(super) fn source_record_with_key(
    schema_id: &str,
    node_id: &str,
    now: u64,
    record_key: Vec<u8>,
    payload: serde_json::Value,
    tombstone: bool,
) -> anyhow::Result<SyncRecord> {
    source_record_with_key_for_subject(
        schema_id, node_id, node_id, now, record_key, payload, tombstone,
    )
}

pub(super) fn source_record_with_key_for_subject(
    schema_id: &str,
    subject_node_id: &str,
    observer_node_id: &str,
    now: u64,
    record_key: Vec<u8>,
    payload: serde_json::Value,
    tombstone: bool,
) -> anyhow::Result<SyncRecord> {
    let mut payload = serde_json::to_vec(&payload)?;
    if payload.len() > MAX_SOURCE_PAYLOAD_BYTES {
        tracing::warn!(
            schema_id,
            payload_bytes = payload.len(),
            "history source observation exceeded its bounded segment payload"
        );
        payload = serde_json::to_vec(&serde_json::json!({
            "truncated": true,
            "observed_at_unix_seconds": now,
        }))?;
    }
    Ok(SyncRecord::new(
        subject_node_id,
        observer_node_id,
        schema_id,
        1,
        record_key,
        payload,
        tombstone,
    ))
}

#[cfg(test)]
mod tests {
    use super::{run_local_source_worker_cycle_with_budget, source_record, source_record_with_key};

    #[test]
    fn source_tombstone_keeps_the_affected_schema_and_marks_the_record() {
        let tombstone = source_record("runtime.v1", "node-a", 100, serde_json::Value::Null, true)
            .expect("tombstone source record");
        assert!(tombstone.is_tombstone());
        assert_eq!(tombstone.schema().0, "runtime.v1");
        assert!(
            !source_record("runtime.v1", "node-a", 100, serde_json::Value::Null, false,)
                .expect("live source record")
                .is_tombstone()
        );
        assert_eq!(
            source_record_with_key(
                "traffic.v1",
                "node-a",
                100,
                b"deleted-history-key".to_vec(),
                serde_json::Value::Null,
                true,
            )
            .expect("stable deletion record")
            .record_key(),
            b"deleted-history-key",
        );
    }

    #[tokio::test]
    async fn source_cycle_does_not_wait_for_stuck_raft_maintenance() {
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            run_local_source_worker_cycle_with_budget(
                std::future::pending::<anyhow::Result<()>>(),
                std::future::pending::<anyhow::Result<()>>(),
                async { Ok(()) },
                std::time::Duration::from_millis(10),
            ),
        )
        .await;

        assert!(
            result.is_ok(),
            "source collection must not be blocked by a Raft maintenance write"
        );
        let (_capacity, _lifecycle, source) = result.expect("worker cycle completed");
        assert!(source.is_ok());
    }
}
