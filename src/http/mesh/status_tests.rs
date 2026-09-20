use std::collections::BTreeMap;

use chrono::{Duration, Utc};

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
    let local = xp_test_fixtures::primary_node_id();
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
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
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
    let now = xp_test_fixtures::baseline_timestamp()
        .parse::<chrono::DateTime<Utc>>()
        .unwrap()
        + Duration::minutes(1);
    let probes = BTreeMap::from([
        (
            local.to_string(),
            NodeEgressProbeState {
                public_ipv4: Some(xp_test_fixtures::primary_ipv4().to_owned()),
                selected_public_ip: Some(xp_test_fixtures::primary_ipv4().to_owned()),
                last_success_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
                ..Default::default()
            },
        ),
        (
            peer.to_string(),
            NodeEgressProbeState {
                public_ipv4: Some(xp_test_fixtures::secondary_ipv4().to_owned()),
                selected_public_ip: Some(xp_test_fixtures::secondary_ipv4().to_owned()),
                last_success_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
                ..Default::default()
            },
        ),
    ]);
    let connections = vec![
        EstablishedTcpConnection {
            local_ip: "0.0.0.0".parse().unwrap(),
            local_port: 44444,
            remote_ip: xp_test_fixtures::secondary_ipv4().parse().unwrap(),
            remote_port: 50001,
        },
        EstablishedTcpConnection {
            local_ip: "0.0.0.0".parse().unwrap(),
            local_port: 44444,
            remote_ip: xp_test_fixtures::secondary_ipv4().parse().unwrap(),
            remote_port: 50002,
        },
        EstablishedTcpConnection {
            local_ip: "0.0.0.0".parse().unwrap(),
            local_port: 44444,
            remote_ip: xp_test_fixtures::secondary_ipv4().parse().unwrap(),
            remote_port: 50003,
        },
        EstablishedTcpConnection {
            local_ip: "0.0.0.0".parse().unwrap(),
            local_port: 44444,
            remote_ip: xp_test_fixtures::tertiary_ipv4().parse().unwrap(),
            remote_port: 50004,
        },
    ];
    let report = build_mesh_connection_usage(
        local,
        &nodes,
        &endpoints,
        &assignments,
        &probes,
        now,
        true,
        None,
        &connections,
    );

    let reverse = report.reverse_by_peer.get(peer).unwrap();
    assert_eq!(reverse.logical_links, 1);
    assert_eq!(reverse.physical_connections, Some(3));
    assert!(matches!(
        reverse.state,
        AdminReverseUnderlayState::OverLimit
    ));
    assert_eq!(report.local.user_inbound.connections, Some(1));
    assert_eq!(report.local.user_inbound.cluster_peer, Some(0));
    assert_eq!(report.local.user_inbound.external, Some(1));
    assert_eq!(report.local.user_inbound.unknown, Some(0));
    assert!(!report.local.user_inbound.sources_truncated);
}

#[test]
fn target_side_standby_link_counts_the_rendezvous_socket() {
    let local = "node-target";
    let peer = "node-standby";
    let now = xp_test_fixtures::baseline_timestamp()
        .parse::<chrono::DateTime<Utc>>()
        .unwrap()
        + Duration::minutes(1);
    let nodes = vec![
        Node {
            node_id: local.to_string(),
            node_name: "target".to_string(),
            access_host: "target.example.test".to_string(),
            api_base_url: "https://target.example.test".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
        Node {
            node_id: peer.to_string(),
            node_name: "standby".to_string(),
            access_host: "standby.example.test".to_string(),
            api_base_url: "https://standby.example.test".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    ];
    let mut peer_meta = xp_test_fixtures::endpoint_vless_meta().clone();
    peer_meta["managed_default"] = serde_json::json!(true);
    peer_meta["transport"] = serde_json::json!("xhttp");
    let endpoints = vec![Endpoint {
        endpoint_id: "endpoint-standby".to_string(),
        node_id: peer.to_string(),
        tag: "standby-vless".to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 44444,
        meta: peer_meta,
    }];
    let assignments = BTreeMap::from([(
        local.to_string(),
        ReverseMeshAssignment {
            target_node_id: local.to_string(),
            generation: 9,
            membership_revision: 1,
            primary_node_id: "node-primary".to_string(),
            standby_node_id: Some(peer.to_string()),
            credential_epoch: 1,
        },
    )]);
    let probes = BTreeMap::from([(
        peer.to_string(),
        NodeEgressProbeState {
            public_ipv4: Some(xp_test_fixtures::address_documentation192_0_2_30().to_owned()),
            selected_public_ip: Some(
                xp_test_fixtures::address_documentation192_0_2_30().to_owned(),
            ),
            last_success_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
            ..Default::default()
        },
    )]);
    let connections = vec![EstablishedTcpConnection {
        local_ip: "0.0.0.0".parse().unwrap(),
        local_port: 51000,
        remote_ip: xp_test_fixtures::address_documentation192_0_2_30()
            .parse()
            .unwrap(),
        remote_port: 44444,
    }];

    let report = build_mesh_connection_usage(
        local,
        &nodes,
        &endpoints,
        &assignments,
        &probes,
        now,
        true,
        None,
        &connections,
    );

    let reverse = report.reverse_by_peer.get(peer).expect("standby peer");
    assert_eq!(reverse.physical_connections, Some(1));
    assert_eq!(reverse.links[0].connections, Some(1));
    assert!(matches!(
        reverse.links[0].role,
        crate::reverse_mesh::ReverseRole::Standby
    ));
}

#[test]
fn unsupported_socket_collection_is_explicitly_unavailable() {
    let report = build_mesh_connection_usage(
        "node-local",
        &[],
        &[],
        &BTreeMap::new(),
        &BTreeMap::new(),
        Utc::now(),
        false,
        Some("socket inspection unavailable".to_string()),
        &[],
    );

    assert!(!report.local.supported);
    assert_eq!(report.local.user_inbound.connections, None);
    assert_eq!(report.local.user_inbound.external, None);
    assert_eq!(report.local.user_inbound.cluster_peer, None);
    assert_eq!(report.local.user_inbound.unknown, None);
    assert!(report.local.user_inbound.sources.is_empty());
}

#[test]
fn stale_egress_addresses_are_not_used_for_peer_classification() {
    let local = xp_test_fixtures::primary_node_id();
    let now = xp_test_fixtures::baseline_timestamp()
        .parse::<chrono::DateTime<Utc>>()
        .unwrap()
        + Duration::hours(2);
    let probes = BTreeMap::from([(
        "peer".to_string(),
        NodeEgressProbeState {
            public_ipv4: Some(xp_test_fixtures::secondary_ipv4().to_owned()),
            selected_public_ip: Some(xp_test_fixtures::secondary_ipv4().to_owned()),
            last_success_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
            ..Default::default()
        },
    )]);
    let connections = vec![EstablishedTcpConnection {
        local_ip: "0.0.0.0".parse().unwrap(),
        local_port: 44444,
        remote_ip: xp_test_fixtures::secondary_ipv4().parse().unwrap(),
        remote_port: 50001,
    }];
    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 44444,
        meta: {
            let mut meta = xp_test_fixtures::endpoint_vless_meta().clone();
            meta["managed_default"] = serde_json::json!(true);
            meta["transport"] = serde_json::json!("xhttp");
            meta
        },
    };
    let report = build_mesh_connection_usage(
        local,
        &[],
        &[endpoint],
        &BTreeMap::new(),
        &probes,
        now,
        true,
        None,
        &connections,
    );

    assert_eq!(report.local.user_inbound.external, Some(0));
    assert_eq!(report.local.user_inbound.cluster_peer, Some(0));
    assert_eq!(report.local.user_inbound.unknown, Some(1));
}

#[test]
fn shared_egress_address_is_unknown_and_ipv6_is_classified() {
    let local = xp_test_fixtures::primary_node_id();
    let now = xp_test_fixtures::baseline_timestamp()
        .parse::<chrono::DateTime<Utc>>()
        .unwrap()
        + Duration::minutes(1);
    let probes = BTreeMap::from([
        (
            "peer-a".to_string(),
            NodeEgressProbeState {
                public_ipv4: Some(xp_test_fixtures::primary_ipv4().to_owned()),
                public_ipv6: Some(xp_test_fixtures::private_ipv6_address().to_owned()),
                selected_public_ip: Some(xp_test_fixtures::private_ipv6_address().to_owned()),
                last_success_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
                ..Default::default()
            },
        ),
        (
            "peer-b".to_string(),
            NodeEgressProbeState {
                public_ipv4: Some(xp_test_fixtures::primary_ipv4().to_owned()),
                selected_public_ip: Some(xp_test_fixtures::primary_ipv4().to_owned()),
                last_success_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
                ..Default::default()
            },
        ),
    ]);
    let connections = vec![
        EstablishedTcpConnection {
            local_ip: "::".parse().unwrap(),
            local_port: 44444,
            remote_ip: xp_test_fixtures::primary_ipv4().parse().unwrap(),
            remote_port: 50001,
        },
        EstablishedTcpConnection {
            local_ip: "::".parse().unwrap(),
            local_port: 44444,
            remote_ip: xp_test_fixtures::private_ipv6_address().parse().unwrap(),
            remote_port: 50002,
        },
    ];
    let mut meta = xp_test_fixtures::endpoint_vless_meta().clone();
    meta["managed_default"] = serde_json::json!(true);
    meta["transport"] = serde_json::json!("xhttp");
    let report = build_mesh_connection_usage(
        local,
        &[],
        &[Endpoint {
            endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 44444,
            meta,
        }],
        &BTreeMap::new(),
        &probes,
        now,
        true,
        None,
        &connections,
    );

    assert_eq!(report.local.user_inbound.unknown, Some(1));
    assert_eq!(report.local.user_inbound.cluster_peer, Some(1));
}

#[test]
fn shared_egress_address_does_not_hide_reverse_or_external_sockets() {
    let local = "node-local";
    let target = "node-target";
    let shared = "node-shared";
    let now = xp_test_fixtures::baseline_timestamp()
        .parse::<chrono::DateTime<Utc>>()
        .unwrap()
        + Duration::minutes(1);
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
            node_id: target.to_string(),
            node_name: "target".to_string(),
            access_host: "target.example.test".to_string(),
            api_base_url: "https://target.example.test".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
        Node {
            node_id: shared.to_string(),
            node_name: "shared".to_string(),
            access_host: "shared.example.test".to_string(),
            api_base_url: "https://shared.example.test".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    ];
    let mut meta = xp_test_fixtures::endpoint_vless_meta().clone();
    meta["managed_default"] = serde_json::json!(true);
    meta["transport"] = serde_json::json!("xhttp");
    let endpoints = vec![Endpoint {
        endpoint_id: "endpoint-local".to_string(),
        node_id: local.to_string(),
        tag: "local-vless".to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 44444,
        meta,
    }];
    let assignments = BTreeMap::from([(
        target.to_string(),
        ReverseMeshAssignment {
            target_node_id: target.to_string(),
            generation: 1,
            membership_revision: 1,
            primary_node_id: local.to_string(),
            standby_node_id: None,
            credential_epoch: 1,
        },
    )]);
    let probes = BTreeMap::from([
        (
            target.to_string(),
            NodeEgressProbeState {
                public_ipv4: Some(xp_test_fixtures::address_documentation192_0_2_30().to_owned()),
                selected_public_ip: Some(
                    xp_test_fixtures::address_documentation192_0_2_30().to_owned(),
                ),
                last_success_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
                ..Default::default()
            },
        ),
        (
            shared.to_string(),
            NodeEgressProbeState {
                public_ipv4: Some(xp_test_fixtures::address_documentation192_0_2_30().to_owned()),
                selected_public_ip: Some(
                    xp_test_fixtures::address_documentation192_0_2_30().to_owned(),
                ),
                last_success_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
                ..Default::default()
            },
        ),
    ]);
    let connections = vec![
        EstablishedTcpConnection {
            local_ip: "0.0.0.0".parse().unwrap(),
            local_port: 44444,
            remote_ip: xp_test_fixtures::address_documentation192_0_2_30()
                .parse()
                .unwrap(),
            remote_port: 50001,
        },
        EstablishedTcpConnection {
            local_ip: "0.0.0.0".parse().unwrap(),
            local_port: 44444,
            remote_ip: xp_test_fixtures::address_documentation192_0_2_32()
                .parse()
                .unwrap(),
            remote_port: 50002,
        },
    ];

    let report = build_mesh_connection_usage(
        local,
        &nodes,
        &endpoints,
        &assignments,
        &probes,
        now,
        true,
        None,
        &connections,
    );

    let reverse = report.reverse_by_peer.get(target).expect("target peer");
    assert_eq!(reverse.physical_connections, None);
    assert_eq!(report.local.user_inbound.connections, Some(2));
    assert_eq!(report.local.user_inbound.unknown, Some(2));
    assert_eq!(report.local.user_inbound.external, Some(0));
}

#[test]
fn inbound_sources_are_bounded_without_losing_category_totals() {
    let local = xp_test_fixtures::primary_node_id();
    let now = xp_test_fixtures::baseline_timestamp()
        .parse::<chrono::DateTime<Utc>>()
        .unwrap();
    let mut meta = xp_test_fixtures::endpoint_vless_meta().clone();
    meta["managed_default"] = serde_json::json!(true);
    meta["transport"] = serde_json::json!("xhttp");
    let connections = (0..(super::MAX_CONNECTION_SOURCES + 7))
        .map(|index| EstablishedTcpConnection {
            local_ip: "0.0.0.0".parse().unwrap(),
            local_port: 44444,
            remote_ip: format!("203.0.113.{}", index + 1).parse().unwrap(),
            remote_port: 50000 + index as u16,
        })
        .collect::<Vec<_>>();
    let report = build_mesh_connection_usage(
        local,
        &[],
        &[Endpoint {
            endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 44444,
            meta,
        }],
        &BTreeMap::new(),
        &BTreeMap::new(),
        now,
        true,
        None,
        &connections,
    );

    assert_eq!(report.local.user_inbound.connections, Some(135));
    assert_eq!(report.local.user_inbound.external, Some(0));
    assert_eq!(report.local.user_inbound.unknown, Some(135));
    assert_eq!(
        report.local.user_inbound.sources.len(),
        super::MAX_CONNECTION_SOURCES
    );
    assert!(report.local.user_inbound.sources_truncated);
}
