use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Durable deletion fact for endpoint-scoped connection history. Connection samples stay outside
/// Raft; only the registry that hides their subjects does.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeletedEndpointRecord {
    pub node_id: String,
    #[serde(default)]
    pub generation: u64,
}

pub type DeletedEndpointRegistry = BTreeMap<String, DeletedEndpointRecord>;

pub(crate) fn record_deleted_endpoint(
    registry: &mut DeletedEndpointRegistry,
    endpoint_id: &str,
    node_id: &str,
) {
    let generation = registry
        .get(endpoint_id)
        .map(|record| record.generation)
        .unwrap_or_default()
        .saturating_add(1);
    registry.insert(
        endpoint_id.to_string(),
        DeletedEndpointRecord {
            node_id: node_id.to_string(),
            generation,
        },
    );
}
