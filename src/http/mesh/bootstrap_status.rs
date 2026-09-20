use std::{collections::BTreeMap, path::Path};

use super::{AdminReverseLinkStatus, AdminReverseUnderlayState, AdminReverseUnderlayStatus};
use crate::reverse_mesh::{ReverseMeshBootstrapMarker, ReverseRole};

pub(crate) fn active_bootstrap_marker(
    store: &crate::state::JsonSnapshotStore,
    data_dir: &Path,
) -> Option<ReverseMeshBootstrapMarker> {
    let assignments = &store.state().reverse_mesh_assignments;
    let target = store
        .state()
        .active_membership_operation()
        .filter(|operation| {
            operation.kind == crate::state::MembershipOperationKind::Join
                && !operation.phase.is_terminal()
        })
        .and_then(|operation| operation.node_id.as_deref())
        .filter(|target| assignments.contains_key(*target));
    crate::raft::http_rpc::read_bootstrap_sender_marker(
        crate::cluster_metadata::ClusterPaths::new(data_dir).raft_bootstrap_sender,
    )
    .and_then(|marker| marker.reverse_mesh)
    .filter(|marker| target == Some(marker.target_node_id.as_str()))
}

pub(super) fn add_bootstrap_status(
    reverse_by_peer: &mut BTreeMap<String, AdminReverseUnderlayStatus>,
    local_node_id: &str,
    marker: Option<&ReverseMeshBootstrapMarker>,
    supported: bool,
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
        let state = if supported {
            AdminReverseUnderlayState::Unknown
        } else {
            AdminReverseUnderlayState::Unavailable
        };
        let link = AdminReverseLinkStatus {
            target_node_id: local_node_id.to_string(),
            rendezvous_node_id: rendezvous_node_id.to_string(),
            role: ReverseRole::Bootstrap,
            generation: marker.generation,
            connections: None,
            limit: 2,
            state,
        };
        if let Some(status) = reverse_by_peer.get_mut(rendezvous_node_id) {
            let mut found = false;
            for link in &mut status.links {
                if link.target_node_id == local_node_id
                    && link.rendezvous_node_id == rendezvous_node_id
                {
                    link.role = ReverseRole::Bootstrap;
                    link.generation = marker.generation;
                    link.connections = None;
                    link.state = state;
                    found = true;
                }
            }
            if !found {
                status.links.push(AdminReverseLinkStatus {
                    target_node_id: local_node_id.to_string(),
                    rendezvous_node_id: rendezvous_node_id.to_string(),
                    role: ReverseRole::Bootstrap,
                    generation: marker.generation,
                    connections: None,
                    limit: 2,
                    state: AdminReverseUnderlayState::Unknown,
                });
                status.logical_links += 1;
            }
            status.physical_connections = None;
            status.state = state;
        } else {
            reverse_by_peer.insert(
                rendezvous_node_id.to_string(),
                AdminReverseUnderlayStatus {
                    logical_links: 1,
                    physical_connections: None,
                    limit_per_link: 2,
                    state,
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
        add_bootstrap_status(&mut status, "target", Some(&marker), true);
        let link = &status["rendezvous"].links[0];
        assert_eq!(status["rendezvous"].logical_links, 2);
        assert!(matches!(link.role, ReverseRole::Bootstrap));
        assert_eq!(status["rendezvous"].physical_connections, None);
    }

    #[test]
    fn bootstrap_status_preserves_unavailable_socket_collection() {
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
        add_bootstrap_status(&mut status, "target", Some(&marker), false);
        assert!(matches!(
            status["rendezvous"].state,
            AdminReverseUnderlayState::Unavailable
        ));
        assert!(matches!(
            status["rendezvous"].links[0].state,
            AdminReverseUnderlayState::Unavailable
        ));
    }
}
