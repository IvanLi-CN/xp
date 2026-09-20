use std::collections::BTreeMap;

use super::{AdminReverseLinkStatus, AdminReverseUnderlayState, AdminReverseUnderlayStatus};
use crate::reverse_mesh::{ReverseMeshBootstrapMarker, ReverseRole};

pub(super) fn add_bootstrap_status(
    reverse_by_peer: &mut BTreeMap<String, AdminReverseUnderlayStatus>,
    local_node_id: &str,
    marker: Option<&ReverseMeshBootstrapMarker>,
) {
    let Some(marker) = marker.filter(|marker| marker.target_node_id == local_node_id) else {
        return;
    };
    for rendezvous_node_id in [
        Some(marker.primary_node_id.as_str()),
        marker.standby_node_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if rendezvous_node_id == local_node_id || reverse_by_peer.contains_key(rendezvous_node_id) {
            continue;
        }
        reverse_by_peer.insert(
            rendezvous_node_id.to_string(),
            AdminReverseUnderlayStatus {
                logical_links: 1,
                physical_connections: None,
                limit_per_link: 2,
                state: AdminReverseUnderlayState::Unknown,
                links: vec![AdminReverseLinkStatus {
                    target_node_id: local_node_id.to_string(),
                    rendezvous_node_id: rendezvous_node_id.to_string(),
                    role: ReverseRole::Bootstrap,
                    generation: marker.generation,
                    connections: None,
                    limit: 2,
                    state: AdminReverseUnderlayState::Unknown,
                }],
            },
        );
    }
}
