use super::*;

pub(super) async fn infer_state_applied(
    paths: &StorePaths,
    meta: &PersistedStateMachineMeta,
) -> bool {
    let Some(last_applied) = meta.last_applied else {
        return false;
    };
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
    if validate_snapshot_payload(&snapshot_meta, &snapshot_bytes).is_err() {
        return false;
    }
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&snapshot_bytes) else {
        return false;
    };
    payload
        .get("mesh_state_applied")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

pub(super) async fn validate_persisted_snapshot(paths: &StorePaths) -> bool {
    let Some(snapshot_meta) =
        read_json::<SnapshotMeta<NodeId, NodeMeta>>(&paths.snapshot_meta_json)
            .await
            .ok()
            .flatten()
    else {
        return false;
    };
    let Ok(snapshot_bytes) = read_bytes(&paths.snapshot_data_json).await else {
        return false;
    };
    if validate_snapshot_payload(&snapshot_meta, &snapshot_bytes).is_err() {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(&snapshot_bytes)
        .ok()
        .and_then(|payload| {
            payload
                .get("mesh_state_applied")
                .and_then(serde_json::Value::as_bool)
        })
        .unwrap_or(false)
}

pub(super) fn validate_snapshot_payload(
    meta: &SnapshotMeta<NodeId, NodeMeta>,
    bytes: &[u8],
) -> Result<(), std::io::Error> {
    let payload: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| std::io::Error::other(format!("invalid snapshot payload: {e}")))?;
    let mesh_state_applied = payload
        .get("mesh_state_applied")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if mesh_state_applied
        && (payload.get("snapshot_id").is_none() || payload.get("last_log_id").is_none())
    {
        return Err(std::io::Error::other(
            "authenticated snapshot payload is missing identity",
        ));
    }
    if let Some(snapshot_id) = payload.get("snapshot_id")
        && snapshot_id.as_str() != Some(meta.snapshot_id.as_str())
    {
        return Err(std::io::Error::other("snapshot payload metadata mismatch"));
    }
    if let Some(last_log_id) = payload.get("last_log_id") {
        let payload_last_log_id =
            serde_json::from_value::<Option<LogId<NodeId>>>(last_log_id.clone())
                .map_err(|e| std::io::Error::other(format!("invalid snapshot payload log: {e}")))?;
        if payload_last_log_id != meta.last_log_id {
            return Err(std::io::Error::other("snapshot payload log mismatch"));
        }
    }
    Ok(())
}

pub(super) fn snapshot_mesh_state_applied(bytes: &[u8]) -> Result<bool, std::io::Error> {
    let payload: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| std::io::Error::other(format!("invalid snapshot payload: {e}")))?;
    Ok(payload
        .get("mesh_state_applied")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false))
}

pub(super) fn normalize_snapshot_payload(
    meta: &SnapshotMeta<NodeId, NodeMeta>,
    mut payload: serde_json::Value,
) -> Result<Vec<u8>, std::io::Error> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("invalid snapshot payload: expected object"))?;
    let mesh_state_applied = object
        .get("mesh_state_applied")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    object.insert(
        "mesh_state_applied".to_string(),
        serde_json::Value::Bool(mesh_state_applied),
    );
    object.insert(
        "snapshot_id".to_string(),
        serde_json::Value::String(meta.snapshot_id.clone()),
    );
    object.insert(
        "last_log_id".to_string(),
        serde_json::to_value(meta.last_log_id).map_err(std::io::Error::other)?,
    );
    serde_json::to_vec_pretty(&payload).map_err(std::io::Error::other)
}
