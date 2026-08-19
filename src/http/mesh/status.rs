use chrono::{DateTime, Utc};

use super::{AdminMeshTransportStatus, mesh_transport_counts_for, mesh_transport_health_for};
use crate::{
    mesh_telemetry::{MeshActiveRoute, MeshPeerTelemetry},
    reverse_mesh::ReverseMeshAssignment,
};

pub(super) fn with_assignment(
    route: Option<MeshActiveRoute>,
    assignment: Option<&ReverseMeshAssignment>,
) -> Option<MeshActiveRoute> {
    route.map(|mut route| {
        if let Some(assignment) = assignment {
            route.primary_rendezvous = Some(assignment.primary_node_id.clone());
            route.standby_rendezvous = assignment.standby_node_id.clone();
            route.generation = Some(assignment.generation);
        }
        route
    })
}

pub(super) fn mesh_transport_status_for(
    mesh_enabled: bool,
    peer: Option<&MeshPeerTelemetry>,
    now: DateTime<Utc>,
) -> Option<AdminMeshTransportStatus> {
    if !mesh_enabled {
        return None;
    }
    let (requests_5m, connection_starts_5m) = peer
        .map(|peer| mesh_transport_counts_for(peer, 5, now))
        .unwrap_or_default();
    let (requests_1h, connection_starts_1h) = peer
        .map(|peer| mesh_transport_counts_for(peer, 60, now))
        .unwrap_or_default();
    Some(AdminMeshTransportStatus {
        protocol: peer.and_then(|peer| peer.last_mesh_protocol),
        health: mesh_transport_health_for(peer, now),
        connection_generation: peer.map_or(0, |peer| peer.connection_generation),
        current_connection_requests: peer.map_or(0, |peer| peer.current_connection_requests),
        requests_5m,
        connection_starts_5m,
        requests_1h,
        connection_starts_1h,
        last_connection_started_at: peer.and_then(|peer| peer.last_connection_started_at.clone()),
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        mesh_telemetry::{ActiveRouteKind, MeshActiveRoute},
        reverse_mesh::ReverseMeshAssignment,
    };

    use super::with_assignment;

    #[test]
    fn assignment_enriches_a_direct_route_without_changing_its_kind() {
        let route = with_assignment(
            Some(MeshActiveRoute {
                kind: ActiveRouteKind::RealityDirect,
                rendezvous: None,
                rendezvous_role: None,
                primary_rendezvous: None,
                standby_rendezvous: None,
                generation: None,
                readiness: None,
            }),
            Some(&ReverseMeshAssignment {
                target_node_id: xp_test_fixtures::primary_node_id().to_owned(),
                generation: 7,
                membership_revision: 1,
                primary_node_id: xp_test_fixtures::secondary_node_id().to_owned(),
                standby_node_id: Some(xp_test_fixtures::tertiary_node_id().to_owned()),
                credential_epoch: 1,
            }),
        )
        .expect("route remains present");

        assert_eq!(route.kind, ActiveRouteKind::RealityDirect);
        assert_eq!(
            route.primary_rendezvous.as_deref(),
            Some(xp_test_fixtures::secondary_node_id())
        );
        assert_eq!(
            route.standby_rendezvous.as_deref(),
            Some(xp_test_fixtures::tertiary_node_id())
        );
        assert_eq!(route.generation, Some(7));
    }
}
