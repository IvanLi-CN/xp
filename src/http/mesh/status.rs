use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    str::FromStr,
};

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::{mesh_transport_counts_for, mesh_transport_health_for};
#[path = "bootstrap_status.rs"]
mod bootstrap_status;
use crate::{
    domain::{Endpoint, EndpointKind, Node},
    managed_default_endpoints::managed_default_vless_endpoint,
    mesh_telemetry::{
        MeshActiveRoute, MeshPeerReason, MeshPeerTelemetry, MeshTransportHealth,
        MeshTransportProtocol,
    },
    node_egress_probe::is_node_egress_probe_stale,
    reverse_mesh::{ReverseMeshAssignment, ReverseMeshBootstrapMarker},
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
    pub(super) connections: Option<u32>,
    pub(super) external: Option<u32>,
    pub(super) cluster_peer: Option<u32>,
    pub(super) unknown: Option<u32>,
    pub(super) sources: Vec<AdminConnectionSource>,
    pub(super) sources_truncated: bool,
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
    pub(super) physical_connections: Option<u32>,
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
    pub(super) connections: Option<u32>,
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
const MAX_CONNECTION_SOURCES: usize = 128;

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
    bootstrap: Option<&ReverseMeshBootstrapMarker>,
) -> MeshConnectionUsageReport {
    let collected = collect_established_tcp_connections();
    let sample_now = Utc::now();
    let (supported, warning, connections) = match collected {
        Ok(connections) => (true, None, connections),
        Err(error) => (false, Some(error.to_string()), Vec::new()),
    };
    build_mesh_connection_usage_with_bootstrap(
        local_node_id,
        nodes,
        endpoints,
        assignments,
        egress_probes,
        sample_now,
        supported,
        warning,
        &connections,
        bootstrap,
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
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
    build_mesh_connection_usage_with_bootstrap(
        local_node_id,
        nodes,
        endpoints,
        assignments,
        egress_probes,
        now,
        supported,
        warning,
        connections,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_mesh_connection_usage_with_bootstrap(
    local_node_id: &str,
    nodes: &[Node],
    endpoints: &[Endpoint],
    assignments: &BTreeMap<String, ReverseMeshAssignment>,
    egress_probes: &BTreeMap<String, NodeEgressProbeState>,
    now: DateTime<Utc>,
    supported: bool,
    warning: Option<String>,
    connections: &[EstablishedTcpConnection],
    bootstrap: Option<&ReverseMeshBootstrapMarker>,
) -> MeshConnectionUsageReport {
    let vless_ports = managed_vless_ports(endpoints);
    let egress_ips = egress_ips_by_node(egress_probes, now);
    let egress_identity_complete = nodes
        .iter()
        .all(|node| egress_ips.contains_key(&node.node_id));
    let reverse_connections = reverse_connections_for_local(
        local_node_id,
        nodes,
        assignments,
        &vless_ports,
        &egress_ips,
        supported,
        connections,
    );
    let user_inbound = user_inbound_status(
        &vless_ports.get(local_node_id).cloned().unwrap_or_default(),
        &egress_ips,
        egress_identity_complete,
        supported,
        connections,
        &reverse_connections,
    );
    let reverse_by_peer: BTreeMap<String, AdminReverseUnderlayStatus> = nodes
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
                        connections: observed,
                        limit: REVERSE_UNDERLAY_LIMIT,
                        state,
                    }
                })
                .collect::<Vec<_>>();
            let physical_connections = links
                .iter()
                .map(|link| link.connections)
                .collect::<Option<Vec<_>>>()
                .map(|connections| connections.into_iter().sum());
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

    let mut reverse_by_peer = reverse_by_peer;
    bootstrap_status::add_bootstrap_status(&mut reverse_by_peer, local_node_id, bootstrap);

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
    now: DateTime<Utc>,
) -> BTreeMap<String, BTreeSet<IpAddr>> {
    probes
        .iter()
        .filter(|(_, probe)| !is_node_egress_probe_stale(probe, now))
        .filter_map(|(node_id, probe)| {
            let mut ips = BTreeSet::new();
            if let Some(value) = probe.selected_public_ip.as_deref()
                && let Ok(ip) = IpAddr::from_str(value)
            {
                ips.insert(ip);
            }
            (!ips.is_empty()).then_some((node_id.clone(), ips))
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

fn reverse_connections_for_local(
    local_node_id: &str,
    nodes: &[Node],
    assignments: &BTreeMap<String, ReverseMeshAssignment>,
    vless_ports: &BTreeMap<String, BTreeSet<u16>>,
    egress_ips: &BTreeMap<String, BTreeSet<IpAddr>>,
    supported: bool,
    connections: &[EstablishedTcpConnection],
) -> BTreeSet<EstablishedTcpConnection> {
    if !supported {
        return BTreeSet::new();
    }
    nodes
        .iter()
        .filter(|node| node.node_id != local_node_id)
        .flat_map(|peer| reverse_links_between(local_node_id, &peer.node_id, assignments))
        .filter_map(|(_, _, target_node_id, rendezvous_node_id)| {
            matching_connections_for_link(
                local_node_id,
                &target_node_id,
                &rendezvous_node_id,
                vless_ports,
                egress_ips,
                connections,
            )
        })
        .flatten()
        .collect()
}

fn matching_connections_for_link(
    local_node_id: &str,
    target_node_id: &str,
    rendezvous_node_id: &str,
    vless_ports: &BTreeMap<String, BTreeSet<u16>>,
    egress_ips: &BTreeMap<String, BTreeSet<IpAddr>>,
    connections: &[EstablishedTcpConnection],
) -> Option<BTreeSet<EstablishedTcpConnection>> {
    if local_node_id == rendezvous_node_id {
        let target_ips = egress_ips.get(target_node_id)?;
        let local_ports = vless_ports.get(local_node_id)?;
        let matched = connections
            .iter()
            .filter(|connection| {
                local_ports.contains(&connection.local_port)
                    && target_ips.contains(&connection.remote_ip)
                    && egress_ip_is_unique_for_node(
                        egress_ips,
                        target_node_id,
                        &connection.remote_ip,
                    )
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        if matched.is_empty()
            && target_ips
                .iter()
                .any(|ip| !egress_ip_is_unique_for_node(egress_ips, target_node_id, ip))
        {
            None
        } else {
            Some(matched)
        }
    } else if local_node_id == target_node_id {
        let rendezvous_ips = egress_ips.get(rendezvous_node_id)?;
        let rendezvous_ports = vless_ports.get(rendezvous_node_id)?;
        let matched = connections
            .iter()
            .filter(|connection| {
                rendezvous_ports.contains(&connection.remote_port)
                    && rendezvous_ips.contains(&connection.remote_ip)
                    && egress_ip_is_unique_for_node(
                        egress_ips,
                        rendezvous_node_id,
                        &connection.remote_ip,
                    )
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        if matched.is_empty()
            && rendezvous_ips
                .iter()
                .any(|ip| !egress_ip_is_unique_for_node(egress_ips, rendezvous_node_id, ip))
        {
            None
        } else {
            Some(matched)
        }
    } else {
        None
    }
}

fn egress_ip_is_unique_for_node(
    egress_ips: &BTreeMap<String, BTreeSet<IpAddr>>,
    expected_node_id: &str,
    ip: &IpAddr,
) -> bool {
    let mut owners = egress_ips
        .iter()
        .filter(|(_, ips)| ips.contains(ip))
        .map(|(node_id, _)| node_id.as_str());
    owners.next() == Some(expected_node_id) && owners.next().is_none()
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
    Some(
        matching_connections_for_link(
            local_node_id,
            target_node_id,
            rendezvous_node_id,
            vless_ports,
            egress_ips,
            connections,
        )?
        .len() as u32,
    )
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
        .any(|link| matches!(link.state, AdminReverseUnderlayState::Unavailable))
    {
        AdminReverseUnderlayState::Unavailable
    } else if !supported
        || links
            .iter()
            .any(|link| matches!(link.state, AdminReverseUnderlayState::Unknown))
    {
        AdminReverseUnderlayState::Unknown
    } else if links
        .iter()
        .any(|link| matches!(link.state, AdminReverseUnderlayState::OverLimit))
    {
        AdminReverseUnderlayState::OverLimit
    } else {
        AdminReverseUnderlayState::Ok
    }
}

fn user_inbound_status(
    local_ports: &BTreeSet<u16>,
    egress_ips: &BTreeMap<String, BTreeSet<IpAddr>>,
    egress_identity_complete: bool,
    supported: bool,
    connections: &[EstablishedTcpConnection],
    reverse_connections: &BTreeSet<EstablishedTcpConnection>,
) -> AdminUserInboundStatus {
    if !supported {
        return AdminUserInboundStatus {
            connections: None,
            external: None,
            cluster_peer: None,
            unknown: None,
            sources: Vec::new(),
            sources_truncated: false,
        };
    }
    let mut cluster_ips = BTreeMap::<IpAddr, BTreeSet<&str>>::new();
    for (node_id, ips) in egress_ips {
        for ip in ips {
            cluster_ips.entry(*ip).or_default().insert(node_id);
        }
    }
    let mut sources = BTreeMap::<(String, AdminConnectionClassification), u32>::new();
    let mut external = 0_u32;
    let mut cluster_peer = 0_u32;
    let mut unknown = 0_u32;
    let mut sources_truncated = false;
    for connection in connections.iter().filter(|connection| {
        local_ports.contains(&connection.local_port) && !reverse_connections.contains(*connection)
    }) {
        let classification = if !egress_identity_complete || cluster_ips.is_empty() {
            unknown += 1;
            AdminConnectionClassification::Unknown
        } else {
            match cluster_ips.get(&connection.remote_ip) {
                Some(nodes) if nodes.len() == 1 => {
                    cluster_peer += 1;
                    AdminConnectionClassification::ClusterPeer
                }
                Some(_) => {
                    unknown += 1;
                    AdminConnectionClassification::Unknown
                }
                None => {
                    external += 1;
                    AdminConnectionClassification::External
                }
            }
        };
        let source_key = (connection.remote_ip.to_string(), classification);
        if let Some(count) = sources.get_mut(&source_key) {
            *count += 1;
        } else if sources.len() < MAX_CONNECTION_SOURCES {
            sources.insert(source_key, 1);
        } else {
            sources_truncated = true;
        }
    }
    AdminUserInboundStatus {
        connections: Some(
            external
                .saturating_add(cluster_peer)
                .saturating_add(unknown),
        ),
        external: Some(external),
        cluster_peer: Some(cluster_peer),
        unknown: Some(unknown),
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
        sources_truncated,
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
#[path = "status_tests.rs"]
mod tests;
