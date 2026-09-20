use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    str::FromStr,
};

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::{mesh_transport_counts_for, mesh_transport_health_for};
use crate::{
    domain::{Endpoint, EndpointKind, Node},
    managed_default_endpoints::managed_default_vless_endpoint,
    mesh_telemetry::{
        MeshActiveRoute, MeshPeerReason, MeshPeerTelemetry, MeshTransportHealth,
        MeshTransportProtocol,
    },
    reverse_mesh::ReverseMeshAssignment,
    state::NodeEgressProbeState,
    tcp_connection_usage::{EstablishedTcpConnection, collect_established_tcp_connections},
};

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminMeshTransportStatus {
    pub(super) protocol: Option<MeshTransportProtocol>,
    pub(super) health: MeshTransportHealth,
    pub(super) connection_generation: u64,
    pub(super) current_connection_requests: u64,
    pub(super) requests_5m: u32,
    pub(super) connection_starts_5m: u32,
    pub(super) requests_1h: u32,
    pub(super) connection_starts_1h: u32,
    pub(super) last_connection_started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminMeshConnectionUsage {
    pub(super) supported: bool,
    pub(super) sampled_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) warning: Option<String>,
    pub(super) user_inbound: AdminUserInboundStatus,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminUserInboundStatus {
    pub(super) connections: u32,
    pub(super) external: u32,
    pub(super) cluster_peer: u32,
    pub(super) unknown: u32,
    pub(super) sources: Vec<AdminConnectionSource>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminConnectionSource {
    pub(super) address: String,
    pub(super) connections: u32,
    pub(super) classification: AdminConnectionClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AdminConnectionClassification {
    ClusterPeer,
    External,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminReverseUnderlayStatus {
    pub(super) logical_links: u32,
    pub(super) physical_connections: u32,
    pub(super) limit_per_link: u32,
    pub(super) state: AdminReverseUnderlayState,
    pub(super) links: Vec<AdminReverseLinkStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminReverseLinkStatus {
    pub(super) target_node_id: String,
    pub(super) rendezvous_node_id: String,
    pub(super) role: crate::reverse_mesh::ReverseRole,
    pub(super) generation: u64,
    pub(super) connections: u32,
    pub(super) limit: u32,
    pub(super) state: AdminReverseUnderlayState,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AdminReverseUnderlayState {
    Ok,
    OverLimit,
    Unknown,
    Unavailable,
}

const REVERSE_UNDERLAY_LIMIT: u32 = 2;

#[derive(Debug)]
pub(super) struct MeshConnectionUsageReport {
    pub local: AdminMeshConnectionUsage,
    pub reverse_by_peer: BTreeMap<String, AdminReverseUnderlayStatus>,
}

pub(super) fn collect_mesh_connection_usage(
    local_node_id: &str,
    nodes: &[Node],
    endpoints: &[Endpoint],
    assignments: &BTreeMap<String, ReverseMeshAssignment>,
    egress_probes: &BTreeMap<String, NodeEgressProbeState>,
    now: DateTime<Utc>,
) -> MeshConnectionUsageReport {
    let collected = collect_established_tcp_connections();
    let (supported, warning, connections) = match collected {
        Ok(connections) => (true, None, connections),
        Err(error) => (false, Some(error.to_string()), Vec::new()),
    };
    build_mesh_connection_usage(
        local_node_id,
        nodes,
        endpoints,
        assignments,
        egress_probes,
        now,
        supported,
        warning,
        &connections,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_mesh_connection_usage(
    local_node_id: &str,
    nodes: &[Node],
    endpoints: &[Endpoint],
    assignments: &BTreeMap<String, ReverseMeshAssignment>,
    egress_probes: &BTreeMap<String, NodeEgressProbeState>,
    now: DateTime<Utc>,
    supported: bool,
    warning: Option<String>,
    connections: &[EstablishedTcpConnection],
) -> MeshConnectionUsageReport {
    let vless_ports = managed_vless_ports(endpoints);
    let egress_ips = egress_ips_by_node(egress_probes);
    let user_inbound = user_inbound_status(
        &vless_ports.get(local_node_id).cloned().unwrap_or_default(),
        &egress_ips,
        supported,
        connections,
    );
    let reverse_by_peer = nodes
        .iter()
        .filter(|node| node.node_id != local_node_id)
        .filter_map(|peer| {
            let links = reverse_links_between(local_node_id, &peer.node_id, assignments);
            if links.is_empty() {
                return None;
            }
            let links = links
                .into_iter()
                .map(|(assignment, role, target_node_id, rendezvous_node_id)| {
                    let observed = physical_connections_for_link(
                        local_node_id,
                        &target_node_id,
                        &rendezvous_node_id,
                        &vless_ports,
                        &egress_ips,
                        supported,
                        connections,
                    );
                    let state = reverse_underlay_state(observed, supported);
                    AdminReverseLinkStatus {
                        target_node_id,
                        rendezvous_node_id,
                        role,
                        generation: assignment.generation,
                        connections: observed.unwrap_or_default(),
                        limit: REVERSE_UNDERLAY_LIMIT,
                        state,
                    }
                })
                .collect::<Vec<_>>();
            let physical_connections = links.iter().map(|link| link.connections).sum::<u32>();
            let state = aggregate_reverse_underlay_state(&links, supported);
            Some((
                peer.node_id.clone(),
                AdminReverseUnderlayStatus {
                    logical_links: links.len() as u32,
                    physical_connections,
                    limit_per_link: REVERSE_UNDERLAY_LIMIT,
                    state,
                    links,
                },
            ))
        })
        .collect();

    MeshConnectionUsageReport {
        local: AdminMeshConnectionUsage {
            supported,
            sampled_at: supported.then(|| now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            warning,
            user_inbound,
        },
        reverse_by_peer,
    }
}

fn managed_vless_ports(endpoints: &[Endpoint]) -> BTreeMap<String, BTreeSet<u16>> {
    let mut ports = BTreeMap::<String, BTreeSet<u16>>::new();
    for endpoint in endpoints {
        if endpoint.kind == EndpointKind::VlessRealityVisionTcp
            && managed_default_vless_endpoint(endpoint).is_some()
        {
            ports
                .entry(endpoint.node_id.clone())
                .or_default()
                .insert(endpoint.port);
        }
    }
    ports
}

fn egress_ips_by_node(
    probes: &BTreeMap<String, NodeEgressProbeState>,
) -> BTreeMap<String, BTreeSet<IpAddr>> {
    probes
        .iter()
        .map(|(node_id, probe)| {
            let mut ips = BTreeSet::new();
            for value in [
                probe.public_ipv4.as_deref(),
                probe.public_ipv6.as_deref(),
                probe.selected_public_ip.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                if let Ok(ip) = IpAddr::from_str(value) {
                    ips.insert(ip);
                }
            }
            (node_id.clone(), ips)
        })
        .collect()
}

fn reverse_links_between<'a>(
    local_node_id: &str,
    peer_node_id: &str,
    assignments: &'a BTreeMap<String, ReverseMeshAssignment>,
) -> Vec<(
    &'a ReverseMeshAssignment,
    crate::reverse_mesh::ReverseRole,
    String,
    String,
)> {
    let mut links = Vec::new();
    for assignment in assignments.values() {
        if assignment.target_node_id == peer_node_id && assignment.primary_node_id == local_node_id
        {
            links.push((
                assignment,
                crate::reverse_mesh::ReverseRole::Primary,
                assignment.target_node_id.clone(),
                local_node_id.to_string(),
            ));
        } else if assignment.target_node_id == peer_node_id
            && assignment.standby_node_id.as_deref() == Some(local_node_id)
        {
            links.push((
                assignment,
                crate::reverse_mesh::ReverseRole::Standby,
                assignment.target_node_id.clone(),
                local_node_id.to_string(),
            ));
        } else if assignment.target_node_id == local_node_id
            && assignment.primary_node_id == peer_node_id
        {
            links.push((
                assignment,
                crate::reverse_mesh::ReverseRole::Primary,
                local_node_id.to_string(),
                assignment.primary_node_id.clone(),
            ));
        } else if assignment.target_node_id == local_node_id
            && assignment.standby_node_id.as_deref() == Some(peer_node_id)
        {
            links.push((
                assignment,
                crate::reverse_mesh::ReverseRole::Standby,
                local_node_id.to_string(),
                peer_node_id.to_string(),
            ));
        }
    }
    links
}

fn physical_connections_for_link(
    local_node_id: &str,
    target_node_id: &str,
    rendezvous_node_id: &str,
    vless_ports: &BTreeMap<String, BTreeSet<u16>>,
    egress_ips: &BTreeMap<String, BTreeSet<IpAddr>>,
    supported: bool,
    connections: &[EstablishedTcpConnection],
) -> Option<u32> {
    if !supported {
        return None;
    }
    let target_ips = egress_ips.get(target_node_id)?;
    let rendezvous_ips = egress_ips.get(rendezvous_node_id)?;
    if local_node_id == rendezvous_node_id {
        let local_ports = vless_ports.get(local_node_id)?;
        Some(
            connections
                .iter()
                .filter(|connection| {
                    local_ports.contains(&connection.local_port)
                        && target_ips.contains(&connection.remote_ip)
                })
                .count() as u32,
        )
    } else if local_node_id == target_node_id {
        let rendezvous_ports = vless_ports.get(rendezvous_node_id)?;
        Some(
            connections
                .iter()
                .filter(|connection| {
                    rendezvous_ports.contains(&connection.remote_port)
                        && rendezvous_ips.contains(&connection.remote_ip)
                })
                .count() as u32,
        )
    } else {
        None
    }
}

fn reverse_underlay_state(observed: Option<u32>, supported: bool) -> AdminReverseUnderlayState {
    let Some(observed) = observed else {
        return if supported {
            AdminReverseUnderlayState::Unknown
        } else {
            AdminReverseUnderlayState::Unavailable
        };
    };
    if observed > REVERSE_UNDERLAY_LIMIT {
        AdminReverseUnderlayState::OverLimit
    } else {
        AdminReverseUnderlayState::Ok
    }
}

fn aggregate_reverse_underlay_state(
    links: &[AdminReverseLinkStatus],
    supported: bool,
) -> AdminReverseUnderlayState {
    if links
        .iter()
        .any(|link| matches!(link.state, AdminReverseUnderlayState::OverLimit))
    {
        AdminReverseUnderlayState::OverLimit
    } else if links
        .iter()
        .any(|link| matches!(link.state, AdminReverseUnderlayState::Unavailable))
    {
        AdminReverseUnderlayState::Unavailable
    } else if !supported
        || links
            .iter()
            .any(|link| matches!(link.state, AdminReverseUnderlayState::Unknown))
    {
        AdminReverseUnderlayState::Unknown
    } else {
        AdminReverseUnderlayState::Ok
    }
}

fn user_inbound_status(
    local_ports: &BTreeSet<u16>,
    egress_ips: &BTreeMap<String, BTreeSet<IpAddr>>,
    supported: bool,
    connections: &[EstablishedTcpConnection],
) -> AdminUserInboundStatus {
    let cluster_ips = egress_ips
        .values()
        .flat_map(|ips| ips.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut sources = BTreeMap::<(String, AdminConnectionClassification), u32>::new();
    let mut external = 0_u32;
    let mut cluster_peer = 0_u32;
    let mut unknown = 0_u32;
    for connection in connections
        .iter()
        .filter(|connection| local_ports.contains(&connection.local_port))
    {
        let classification = if !supported || cluster_ips.is_empty() {
            unknown += 1;
            AdminConnectionClassification::Unknown
        } else if cluster_ips.contains(&connection.remote_ip) {
            cluster_peer += 1;
            AdminConnectionClassification::ClusterPeer
        } else {
            external += 1;
            AdminConnectionClassification::External
        };
        *sources
            .entry((connection.remote_ip.to_string(), classification))
            .or_default() += 1;
    }
    AdminUserInboundStatus {
        connections: external
            .saturating_add(cluster_peer)
            .saturating_add(unknown),
        external,
        cluster_peer,
        unknown,
        sources: sources
            .into_iter()
            .map(
                |((address, classification), connections)| AdminConnectionSource {
                    address,
                    connections,
                    classification,
                },
            )
            .collect(),
    }
}

/// Keep the status wire enum compatible with the fixed 3.22/3.21/3.20 Web window.
pub(super) fn status_mesh_reason(reason: MeshPeerReason) -> MeshPeerReason {
    match reason {
        MeshPeerReason::UnsupportedTransport => MeshPeerReason::InvalidAccessHost,
        reason => reason,
    }
}

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

pub(super) fn is_mesh_peer_stale(peer: Option<&MeshPeerTelemetry>, now: DateTime<Utc>) -> bool {
    peer.and_then(|peer| peer.last_sample_at.as_deref())
        .and_then(|sample| DateTime::parse_from_rfc3339(sample).ok())
        .is_some_and(|sample| {
            now.signed_duration_since(sample.with_timezone(&Utc)) > chrono::Duration::minutes(3)
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
    use std::collections::BTreeMap;

    use chrono::Utc;

    use crate::{
        domain::{Endpoint, EndpointKind, Node, NodeQuotaReset},
        mesh_telemetry::{ActiveRouteKind, MeshActiveRoute, MeshPeerReason},
        reverse_mesh::ReverseMeshAssignment,
        state::NodeEgressProbeState,
        tcp_connection_usage::EstablishedTcpConnection,
    };

    use super::{AdminReverseUnderlayState, build_mesh_connection_usage, with_assignment};

    #[test]
    fn unsupported_transport_keeps_the_legacy_status_reason() {
        let reason = super::status_mesh_reason(MeshPeerReason::UnsupportedTransport);
        assert_eq!(reason, MeshPeerReason::InvalidAccessHost);
        assert_eq!(serde_json::to_value(reason).unwrap(), "invalid_access_host");
    }

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

    #[test]
    fn connection_usage_separates_reverse_peers_from_external_users() {
        let local = "node-local";
        let peer = "node-peer";
        let nodes = vec![
            Node {
                node_id: local.to_string(),
                node_name: "local".to_string(),
                access_host: "local.example.test".to_string(),
                api_base_url: "https://local.example.test".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            },
            Node {
                node_id: peer.to_string(),
                node_name: "peer".to_string(),
                access_host: "peer.example.test".to_string(),
                api_base_url: "https://peer.example.test".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            },
        ];
        let mut local_meta = xp_test_fixtures::endpoint_vless_meta().clone();
        local_meta["managed_default"] = serde_json::json!(true);
        local_meta["transport"] = serde_json::json!("xhttp");
        let endpoints = vec![Endpoint {
            endpoint_id: "endpoint-local".to_string(),
            node_id: local.to_string(),
            tag: "local-vless".to_string(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 44444,
            meta: local_meta,
        }];
        let assignments = BTreeMap::from([(
            peer.to_string(),
            ReverseMeshAssignment {
                target_node_id: peer.to_string(),
                generation: 4,
                membership_revision: 1,
                primary_node_id: local.to_string(),
                standby_node_id: None,
                credential_epoch: 1,
            },
        )]);
        let probes = BTreeMap::from([
            (
                local.to_string(),
                NodeEgressProbeState {
                    public_ipv4: Some("198.51.100.10".to_string()),
                    ..Default::default()
                },
            ),
            (
                peer.to_string(),
                NodeEgressProbeState {
                    public_ipv4: Some("198.51.100.20".to_string()),
                    ..Default::default()
                },
            ),
        ]);
        let connections = vec![
            EstablishedTcpConnection {
                local_ip: "0.0.0.0".parse().unwrap(),
                local_port: 44444,
                remote_ip: "198.51.100.20".parse().unwrap(),
                remote_port: 50001,
            },
            EstablishedTcpConnection {
                local_ip: "0.0.0.0".parse().unwrap(),
                local_port: 44444,
                remote_ip: "198.51.100.20".parse().unwrap(),
                remote_port: 50002,
            },
            EstablishedTcpConnection {
                local_ip: "0.0.0.0".parse().unwrap(),
                local_port: 44444,
                remote_ip: "198.51.100.20".parse().unwrap(),
                remote_port: 50003,
            },
            EstablishedTcpConnection {
                local_ip: "0.0.0.0".parse().unwrap(),
                local_port: 44444,
                remote_ip: "203.0.113.24".parse().unwrap(),
                remote_port: 50004,
            },
        ];
        let report = build_mesh_connection_usage(
            local,
            &nodes,
            &endpoints,
            &assignments,
            &probes,
            Utc::now(),
            true,
            None,
            &connections,
        );

        let reverse = report.reverse_by_peer.get(peer).unwrap();
        assert_eq!(reverse.logical_links, 1);
        assert_eq!(reverse.physical_connections, 3);
        assert!(matches!(
            reverse.state,
            AdminReverseUnderlayState::OverLimit
        ));
        assert_eq!(report.local.user_inbound.connections, 4);
        assert_eq!(report.local.user_inbound.cluster_peer, 3);
        assert_eq!(report.local.user_inbound.external, 1);
        assert_eq!(report.local.user_inbound.unknown, 0);
    }
}
