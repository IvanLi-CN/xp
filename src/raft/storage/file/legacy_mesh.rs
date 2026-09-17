use super::*;

pub(super) async fn infer_state_applied(
    paths: &StorePaths,
    meta: &PersistedStateMachineMeta,
) -> bool {
    let Some(last_applied) = meta.last_applied else {
        return false;
    };
    if read_json::<PersistedWal>(&paths.wal_json)
        .await
        .ok()
        .flatten()
        .and_then(|wal| {
            wal.entries
                .into_iter()
                .find(|entry| entry.log_id == last_applied)
        })
        .is_some_and(|entry| matches!(entry.payload, EntryPayload::Normal(_)))
    {
        return true;
    }

    // Snapshot metadata is written for locally-built snapshots as well as installed snapshots,
    // so its log id alone is not evidence that authenticated state was applied. Trust only the
    // explicit payload marker emitted by newer builders, and require both files to describe the
    // same applied log. Missing/legacy markers stay fail-closed.
    let Some(snapshot_meta) =
        read_json::<SnapshotMeta<NodeId, NodeMeta>>(&paths.snapshot_meta_json)
            .await
            .ok()
            .flatten()
    else {
        return false;
    };
    if snapshot_meta.last_log_id != Some(last_applied) {
        return false;
    }
    let Ok(snapshot_bytes) = read_bytes(&paths.snapshot_data_json).await else {
        return false;
    };
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&snapshot_bytes) else {
        return false;
    };
    payload
        .get("mesh_state_applied")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}
