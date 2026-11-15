use super::*;

pub(crate) fn resolve_local_node_id(config: &Config, store: &JsonSnapshotStore) -> Option<String> {
    let nodes = store.list_nodes();
    let resolved = nodes
        .iter()
        .find(|node| node.api_base_url == config.api_base_url)
        .or_else(|| nodes.iter().find(|node| node.node_name == config.node_name))
        .map(|node| node.node_id.clone());
    if resolved.is_some() {
        return resolved;
    }

    // The local metadata is the authoritative identity during bootstrap and after
    // membership restoration. Keep reconciliation alive when the replicated node
    // table has not yet caught up with that identity.
    let metadata_path = config.data_dir.join("cluster").join("metadata.json");
    let bytes = std::fs::read(metadata_path).ok()?;
    let metadata: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    metadata
        .get("node_id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}
