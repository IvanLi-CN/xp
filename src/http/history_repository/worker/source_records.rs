use super::*;

pub(super) struct SourceRecordBatch {
    records: Option<Vec<SyncRecord>>,
    pub(super) deletion_markers: Vec<crate::node_history::RepositoryHistoryDeletionMarker>,
    pub(super) uptime_observation_ids: Vec<String>,
}

impl SourceRecordBatch {
    pub(super) fn take_records(&mut self) -> Vec<SyncRecord> {
        self.records.take().unwrap_or_default()
    }

    pub(super) async fn mark_uptime_observations_enqueued(
        &self,
        state: &AppState,
    ) -> anyhow::Result<()> {
        state
            .uptime
            .mark_enqueued(&self.uptime_observation_ids)
            .await?;
        Ok(())
    }
}

pub(super) async fn source_records(
    state: &AppState,
    now: u64,
) -> anyhow::Result<SourceRecordBatch> {
    let runtime = state.node_runtime.snapshot(50).await;
    let history = state.node_history.snapshot(&state.cluster.node_id).await;
    let mesh = state.mesh_telemetry.snapshot().await;
    let deletion_markers = state
        .node_history
        .repository_deletion_markers(&state.cluster.node_id)
        .await;
    let pending_uptime_observations = state.uptime.pending(512).await?;
    let (inbound_ip, connections) = {
        let store = state.store.lock().await;
        let inbound_ip = serde_json::json!({
            "generated_at": store.inbound_ip_usage().generated_at,
            "latest_minute": store.inbound_ip_usage().latest_minute,
            "online_stats_unavailable": store.inbound_ip_usage().online_stats_unavailable,
            "memberships": store.inbound_ip_usage().memberships.values()
                .filter(|membership| membership.node_id == state.cluster.node_id)
                .take(MAX_SOURCE_SUMMARY_ITEMS)
                .map(|membership| serde_json::json!({
                    "user_id": membership.user_id,
                    "endpoint_id": membership.endpoint_id,
                    "endpoint_tag": membership.endpoint_tag,
                    "ip_count": membership.ips.len(),
                    "last_seen_at": membership.ips.values()
                        .map(|record| record.last_seen_at.as_str())
                        .max(),
                }))
                .collect::<Vec<_>>(),
        });
        let connections = serde_json::json!({
            "generated_at": store.tcp_connection_usage().generated_at,
            "latest_minute": store.tcp_connection_usage().latest_minute,
            "linux_only": store.tcp_connection_usage().linux_only,
            "endpoints": store.tcp_connection_usage().endpoints.values()
                .filter(|endpoint| endpoint.node_id == state.cluster.node_id)
                .take(MAX_SOURCE_SUMMARY_ITEMS)
                .map(|endpoint| serde_json::json!({
                    "endpoint_id": endpoint.endpoint_id,
                    "endpoint_tag": endpoint.endpoint_tag,
                    "port": endpoint.port,
                    "count": endpoint.counts.last().copied().unwrap_or_default(),
                }))
                .collect::<Vec<_>>(),
        });
        (inbound_ip, connections)
    };
    let traffic = history.as_ref().map(|history| {
        serde_json::json!({
            "last_synced_at": history.last_synced_at,
            "last_sync_error": history.last_sync_error,
            "last_five_minute": history.traffic.as_ref()
                .and_then(|traffic| traffic.five_minute.last()),
            "last_daily": history.traffic.as_ref()
                .and_then(|traffic| traffic.daily.last()),
        })
    });
    let path_health = serde_json::json!({
        "generated_at": mesh.generated_at,
        "peers": mesh.peers.into_iter().take(MAX_SOURCE_SUMMARY_ITEMS).collect::<Vec<_>>(),
    });
    let mut live_records = [
        source_record(
            "runtime.v1",
            &state.cluster.node_id,
            now,
            serde_json::to_value(runtime)?,
            false,
        ),
        source_record(
            "traffic.v1",
            &state.cluster.node_id,
            now,
            traffic.unwrap_or(serde_json::Value::Null),
            false,
        ),
        source_record(
            "path_health.v1",
            &state.cluster.node_id,
            now,
            path_health,
            false,
        ),
        source_record(
            "ip_usage.v1",
            &state.cluster.node_id,
            now,
            inbound_ip,
            false,
        ),
        source_record(
            "connections.v1",
            &state.cluster.node_id,
            now,
            connections,
            false,
        ),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    if let Some(history) = history.as_ref() {
        for user_id in &history.user_traffic_users {
            live_records.push(source_record_with_key(
                "traffic.v1",
                &state.cluster.node_id,
                now,
                format!("node-history:user:{user_id}:current").into_bytes(),
                serde_json::json!({
                    "user_id": user_id,
                    "node_id": state.cluster.node_id,
                    "observed_at_unix_seconds": now,
                }),
                false,
            )?);
        }
    }
    let records = source_records_with_deletions(
        &state.cluster.node_id,
        now,
        deletion_markers
            .iter()
            .map(|marker| {
                (
                    marker.schema_id.clone(),
                    marker.record_key.clone(),
                    marker.target_node_id().map(str::to_owned),
                )
            })
            .collect(),
        live_records,
    )?;
    let mut records = records;
    for pending in &pending_uptime_observations {
        let payload =
            crate::uptime_monitor::UptimeHistoryPayload::from_observation(&pending.observation);
        records.push(source_record_with_key_for_subject(
            crate::uptime_monitor::UPTIME_HISTORY_SCHEMA,
            &pending.observation.monitor_id,
            &pending.observation.observer_node_id,
            pending.observation.observed_at_unix_seconds,
            pending.id.as_bytes().to_vec(),
            serde_json::to_value(payload)?,
            false,
        )?);
    }
    Ok(SourceRecordBatch {
        records: Some(records),
        deletion_markers,
        uptime_observation_ids: pending_uptime_observations
            .into_iter()
            .map(|pending| pending.id)
            .collect(),
    })
}

pub(super) fn source_records_with_deletions(
    node_id: &str,
    now: u64,
    deletion_markers: Vec<(String, Vec<u8>, Option<String>)>,
    live_records: Vec<SyncRecord>,
) -> anyhow::Result<Vec<SyncRecord>> {
    let mut tombstones = deletion_markers
        .into_iter()
        .map(|(schema_id, record_key, target_node_id)| {
            let target_node_id = target_node_id.as_deref().unwrap_or(node_id);
            source_record_with_key_for_subject(
                &schema_id,
                target_node_id,
                target_node_id,
                now,
                record_key,
                serde_json::json!({ "deleted_at_unix_seconds": now }),
                true,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    tombstones.extend(live_records);
    Ok(tombstones)
}
