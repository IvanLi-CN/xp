use super::*;

pub(super) async fn historical_source_backfill_records(
    state: &AppState,
    page_cursor: Option<&str>,
    limit: usize,
) -> anyhow::Result<HistoricalBackfillCollector> {
    let cursor = page_cursor
        .map(HistoricalBackfillPageCursor::decode)
        .transpose()?;
    // A bounded historical snapshot must finish even while live sampling continues.
    let snapshot_end_unix_seconds = cursor
        .as_ref()
        .and_then(|cursor| cursor.snapshot_end_unix_seconds)
        .unwrap_or_else(|| u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default());
    let mut records = HistoricalBackfillCollector::new(cursor.map(|cursor| cursor.after), limit)
        .with_snapshot_end(snapshot_end_unix_seconds);
    if let Some(history) = state
        .node_history
        .repository_snapshot(&state.cluster.node_id)
        .await
    {
        let history_node_id = history.node_id.clone();
        for traffic in history.daily_traffic {
            push_backfill_record(
                &mut records,
                "traffic.v1",
                &state.cluster.node_id,
                &format!("{}T00:00:00Z", traffic.date),
                format!(
                    "node-history:node:{history_node_id}:daily-traffic:{}",
                    traffic.date
                ),
                serde_json::to_value(traffic)?,
            )?;
        }
        for status in history.daily_component_status {
            let date = status.date.clone();
            push_backfill_record(
                &mut records,
                "runtime.v1",
                &state.cluster.node_id,
                &format!("{date}T00:00:00Z"),
                format!("node-history:node:{history_node_id}:daily-status:{date}"),
                serde_json::to_value(status)?,
            )?;
        }
        for event in history.component_status_events {
            let occurred_at = event.occurred_at.clone();
            let key = event.event_id.clone();
            push_backfill_record(
                &mut records,
                "path_health.v1",
                &state.cluster.node_id,
                &occurred_at,
                format!("node-history:node:{history_node_id}:event:{key}"),
                serde_json::to_value(event)?,
            )?;
        }
        if let Some(traffic) = history.traffic {
            for bucket in traffic.five_minute {
                let at = bucket.end_at.clone();
                let key = bucket.start_at.clone();
                push_backfill_record(
                    &mut records,
                    "traffic.v1",
                    &state.cluster.node_id,
                    &at,
                    format!("node-history:node:{history_node_id}:five-minute:{key}"),
                    serde_json::to_value(bucket)?,
                )?;
            }
            for bucket in traffic.daily {
                let date = bucket.date.clone();
                push_backfill_record(
                    &mut records,
                    "traffic.v1",
                    &state.cluster.node_id,
                    &format!("{date}T00:00:00Z"),
                    format!("node-history:node:{history_node_id}:daily-rollup:{date}"),
                    serde_json::to_value(bucket)?,
                )?;
            }
        }
    }
    mesh::collect_mesh_history(&state.mesh_telemetry, &state.cluster.node_id, &mut records).await?;
    let store = state.store.lock().await;
    for sample in store
        .inbound_ip_usage()
        .repository_samples_for_node(&state.cluster.node_id)
    {
        let minute = sample.minute.clone();
        push_backfill_record(
            &mut records,
            "ip_usage.v1",
            &state.cluster.node_id,
            &minute,
            format!(
                "node-history:node:{}:inbound-ip:{minute}",
                state.cluster.node_id
            ),
            serde_json::to_value(sample)?,
        )?;
    }
    for endpoint in store
        .tcp_connection_usage()
        .repository_samples_for_node_iter(&state.cluster.node_id)
    {
        let endpoint_id = endpoint.endpoint_id.clone();
        for sample in endpoint.series {
            let minute = sample.minute.clone();
            push_backfill_record(
                &mut records,
                "connections.v1",
                &state.cluster.node_id,
                &minute,
                format!(
                    "node-history:node:{}:tcp:{endpoint_id}:{minute}",
                    state.cluster.node_id
                ),
                serde_json::to_value(sample)?,
            )?;
        }
    }
    drop(store);
    // A node can be asked for its historical window while a deletion is still waiting for its
    // first repository acknowledgement. Include the independent tombstone stream in that same
    // bounded export so a joining repository never treats the old window as resurrectable.
    let now_unix_seconds = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
    state
        .node_history
        .visit_repository_deletion_markers(&state.cluster.node_id, false, |marker| {
            let target = marker
                .target_node_id()
                .unwrap_or(&state.cluster.node_id)
                .to_owned();
            let tombstone = source_record_with_key_for_subject(
                &marker.schema_id,
                &target,
                &target,
                now_unix_seconds,
                marker.record_key,
                serde_json::json!({"deleted_at_unix_seconds": now_unix_seconds}),
                true,
            )?;
            records.push_with_times(0, now_unix_seconds, tombstone)
        })
        .await?;
    Ok(records)
}
