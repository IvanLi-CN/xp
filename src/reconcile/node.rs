use super::*;

pub(crate) fn resolve_local_node_id(config: &Config, store: &JsonSnapshotStore) -> Option<String> {
    let nodes = store.list_nodes();
    nodes
        .iter()
        .find(|node| node.api_base_url == config.api_base_url)
        .or_else(|| nodes.iter().find(|node| node.node_name == config.node_name))
        .map(|node| node.node_id.clone())
}
