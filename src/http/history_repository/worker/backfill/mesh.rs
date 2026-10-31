use super::*;
use crate::mesh_telemetry::MeshTelemetryHandle;

pub(super) async fn collect_mesh_history(
    telemetry: &MeshTelemetryHandle,
    node_id: &str,
    records: &mut HistoricalBackfillCollector,
) -> anyhow::Result<()> {
    telemetry
        .visit_history_buckets(|peer_id, bucket| {
            // Normal telemetry limits latency samples to 64. Reject malformed persisted values
            // before serialization rather than allocating an unbounded temporary payload.
            if peer_id.len() > 1024
                || bucket.minute.len() > 1024
                || bucket.latency_samples_ms.len() > 64
            {
                anyhow::bail!("persisted mesh history bucket exceeds bounded field limits");
            }
            push_backfill_record(
                records,
                "path_health.v1",
                node_id,
                &bucket.minute,
                format!(
                    "node-history:node:{node_id}:mesh:{peer_id}:{}",
                    bucket.minute
                ),
                serde_json::to_value(bucket)?,
            )
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[tokio::test]
    async fn all_peers_and_retained_buckets_are_exported_across_pages() {
        let temp = tempfile::tempdir().unwrap();
        let mesh_dir = temp.path().join("mesh");
        std::fs::create_dir(&mesh_dir).unwrap();
        let mut peers = serde_json::Map::new();
        for index in 0..20 {
            let id = format!("peer-{index:02}");
            peers.insert(
                id.clone(),
                serde_json::json!({
                    "peer_id": id,
                    "buckets": [
                        {"minute": "2026-01-01T00:00:00Z"},
                        {"minute": "2026-01-01T00:01:00Z"}
                    ]
                }),
            );
        }
        std::fs::write(
            mesh_dir.join("telemetry.json"),
            serde_json::to_vec(&serde_json::json!({"schema_version": 1, "peers": peers})).unwrap(),
        )
        .unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        let mut cursor = None;
        let mut keys = BTreeSet::new();
        loop {
            let mut page = HistoricalBackfillCollector::new(cursor, 7);
            collect_mesh_history(&telemetry, "node-a", &mut page)
                .await
                .unwrap();
            assert!(page.records.len() <= 7);
            for (_, record) in page.records.values() {
                assert!(keys.insert(record.record_key().to_vec()));
            }
            if !page.has_more {
                break;
            }
            cursor = Some(page.records.last_key_value().unwrap().0.clone());
        }
        assert_eq!(keys.len(), 40);
    }
}
