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
        if rendezvous_node_id == local_node_id {
            continue;
        }
        let link = AdminReverseLinkStatus {
            target_node_id: local_node_id.to_string(),
            rendezvous_node_id: rendezvous_node_id.to_string(),
            role: ReverseRole::Bootstrap,
            generation: marker.generation,
            connections: None,
            limit: 2,
            state: AdminReverseUnderlayState::Unknown,
        };
        if let Some(status) = reverse_by_peer.get_mut(rendezvous_node_id) {
            status.logical_links += 1;
            status.physical_connections = None;
            status.state = AdminReverseUnderlayState::Unknown;
            status.links.push(link);
        } else {
            reverse_by_peer.insert(
                rendezvous_node_id.to_string(),
                AdminReverseUnderlayStatus {
                    logical_links: 1,
                    physical_connections: None,
                    limit_per_link: 2,
                    state: AdminReverseUnderlayState::Unknown,
                    links: vec![link],
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reverse_mesh::ReverseMeshBootstrapEndpoint;

    #[test]
    fn bootstrap_status_is_visible_alongside_a_formal_link() {
        let marker = ReverseMeshBootstrapMarker {
            epoch: 4,
            generation: 9,
            target_node_id: "target".to_string(),
            primary_node_id: "rendezvous".to_string(),
            standby_node_id: None,
            primary_endpoint: ReverseMeshBootstrapEndpoint {
                access_host: "rendezvous.example.test".to_string(),
                port: 443,
                server_name: "rendezvous.example.test".to_string(),
                public_key: "public-key".to_string(),
                short_id: "short-id".to_string(),
                transport: "xhttp".to_string(),
            },
            standby_endpoint: None,
        };
        let mut status = BTreeMap::new();
        status.insert(
            "rendezvous".to_string(),
            AdminReverseUnderlayStatus {
                logical_links: 1,
                physical_connections: Some(1),
                limit_per_link: 2,
                state: AdminReverseUnderlayState::Ok,
                links: Vec::new(),
            },
        );
        add_bootstrap_status(&mut status, "target", Some(&marker));
        let link = &status["rendezvous"].links[0];
        assert_eq!(status["rendezvous"].logical_links, 2);
        assert!(matches!(link.role, ReverseRole::Bootstrap));
        assert_eq!(status["rendezvous"].physical_connections, None);
    }
}
