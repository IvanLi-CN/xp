use super::*;
use axum::{Router, extract::State, http::StatusCode, routing::post};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::task::JoinHandle;

async fn count_reverse_relay(State(requests): State<Arc<AtomicUsize>>) -> StatusCode {
    requests.fetch_add(1, Ordering::SeqCst);
    StatusCode::SERVICE_UNAVAILABLE
}

async fn spawn_reverse_relay_counter() -> (String, Arc<AtomicUsize>, JoinHandle<()>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/api/admin/_internal/mesh/reverse-relay",
            post(count_reverse_relay),
        )
        .with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reverse relay listener");
    let address = listener.local_addr().expect("reverse relay address");
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}"), requests, task)
}

fn reverse_request() -> MeshRequest {
    MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/mesh/health".to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(2),
        allow_ambiguous_fallback: true,
        request_id: xp_test_fixtures::primary_node_id().to_owned(),
        route: InternalRoute::HealthV2,
        cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
        sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
        updates_active_path: false,
    }
}

fn reverse_assignment() -> ReverseMeshAssignment {
    ReverseMeshAssignment {
        target_node_id: xp_test_fixtures::primary_node_id().to_owned(),
        generation: 1,
        membership_revision: 1,
        primary_node_id: xp_test_fixtures::secondary_node_id().to_owned(),
        standby_node_id: None,
        credential_epoch: 1,
    }
}

fn primary_reverse_target(
    mesh_base_url: Option<String>,
    public_base_url: String,
) -> MeshPeerTarget {
    MeshPeerTarget {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        mesh_base_url,
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url,
    }
}

fn secondary_reverse_target(
    mesh_base_url: Option<String>,
    public_base_url: String,
) -> MeshPeerTarget {
    MeshPeerTarget {
        node_id: xp_test_fixtures::secondary_node_id().to_owned(),
        node_name: xp_test_fixtures::secondary_node_name().to_owned(),
        mesh_base_url,
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url,
    }
}

fn tertiary_reverse_target(
    mesh_base_url: Option<String>,
    public_base_url: String,
) -> MeshPeerTarget {
    MeshPeerTarget {
        node_id: xp_test_fixtures::tertiary_node_id().to_owned(),
        node_name: xp_test_fixtures::tertiary_node_name().to_owned(),
        mesh_base_url,
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url,
    }
}

fn reverse_route(
    rendezvous: MeshPeerTarget,
    standby_rendezvous: Option<MeshPeerTarget>,
    assignment: ReverseMeshAssignment,
) -> ReverseRelayRoute {
    ReverseRelayRoute {
        rendezvous,
        standby_rendezvous,
        assignment,
        role: ReverseRole::Primary,
    }
}

fn peer_node() -> Node {
    Node {
        node_id: xp_test_fixtures::label_peer_a().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        access_host: xp_test_fixtures::label_peer_afixture_test().to_owned(),
        api_base_url: xp_test_fixtures::url_https_public_peer_afixture_test().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: Default::default(),
    }
}

fn managed_vless_endpoint(_endpoint_id: &str, port: u16) -> Endpoint {
    Endpoint {
        endpoint_id: xp_test_fixtures::label_ss1().to_owned(),
        node_id: xp_test_fixtures::label_peer_a().to_owned(),
        tag: xp_test_fixtures::endpoint_tag_fixture507().to_owned(),
        kind: crate::domain::EndpointKind::VlessRealityVisionTcp,
        port,
        meta: serde_json::json!({
            "reality": xp_test_fixtures::endpoint_reality(),
            "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
            "short_ids": xp_test_fixtures::endpoint_short_ids(),
            "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
            "managed_default": true
        }),
    }
}

#[test]
fn peer_target_uses_mesh_only_for_one_managed_default_endpoint() {
    let node = peer_node();
    let unique = peer_target_from_node(&node, &[managed_vless_endpoint("one", 443)]);
    assert_eq!(
        unique.mesh_base_url.as_deref(),
        Some(
            format!(
                "https://{}:443",
                xp_test_fixtures::label_peer_afixture_test()
            )
            .as_str()
        )
    );
    assert_eq!(unique.mesh_reason, MeshPeerReason::MeshAvailable);
    assert_eq!(unique.public_base_url, node.api_base_url);
    let missing = peer_target_from_node(&node, &[]);
    assert!(missing.mesh_base_url.is_none());
    assert_eq!(missing.mesh_reason, MeshPeerReason::MissingEndpoint);
    let missing_access_host = Node {
        access_host: xp_test_fixtures::label_empty().to_owned(),
        ..node.clone()
    };
    assert!(
        peer_target_from_node(&missing_access_host, &[managed_vless_endpoint("one", 443)],)
            .mesh_base_url
            .is_none()
    );
    assert_eq!(
        peer_target_from_node(&missing_access_host, &[managed_vless_endpoint("one", 443)])
            .mesh_reason,
        MeshPeerReason::InvalidAccessHost
    );
    let invalid_access_host = Node {
        access_host: xp_test_fixtures::address_loopback().to_owned(),
        ..node.clone()
    };
    assert!(
        peer_target_from_node(&invalid_access_host, &[managed_vless_endpoint("one", 443)],)
            .mesh_base_url
            .is_none()
    );
    assert_eq!(
        peer_target_from_node(&invalid_access_host, &[managed_vless_endpoint("one", 443)])
            .mesh_reason,
        MeshPeerReason::InvalidAccessHost
    );
    let absolute_fqdn = Node {
        access_host: xp_test_fixtures::label_peer_afixture_test_variant2().to_owned(),
        ..node.clone()
    };
    assert_eq!(
        peer_target_from_node(&absolute_fqdn, &[managed_vless_endpoint("one", 443)],)
            .mesh_base_url
            .as_deref(),
        Some(
            format!(
                "https://{}:443",
                xp_test_fixtures::label_peer_afixture_test()
            )
            .as_str()
        )
    );
    let ambiguous = peer_target_from_node(
        &node,
        &[
            managed_vless_endpoint("one", 443),
            managed_vless_endpoint("two", 8443),
        ],
    );
    assert!(ambiguous.mesh_base_url.is_none());
    assert_eq!(ambiguous.mesh_reason, MeshPeerReason::AmbiguousEndpoint);
}

#[tokio::test]
async fn reverse_only_request_respects_the_local_readiness_gate() {
    let gate = Arc::new(AtomicBool::new(false));
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_reverse_gate(gate);
    let error = client
        .send_peer_reverse_request(
            &peer_target_from_node(&peer_node(), &[managed_vless_endpoint("one", 443)]),
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_string(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(xp_test_fixtures::number_value1()),
                allow_ambiguous_fallback: true,
                request_id: xp_test_fixtures::primary_node_id().to_owned(),
                route: InternalRoute::MeshV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::secondary_node_id().to_owned(),
                updates_active_path: true,
            },
            "",
            "",
        )
        .await
        .expect_err("disabled Reverse must not resolve or send a route");

    assert!(error.to_string().contains("reverse relay is disabled"));
}

#[tokio::test]
async fn reverse_outer_request_prefers_rendezvous_reality_mesh() {
    let (mesh_base_url, mesh_requests, mesh_task) = spawn_reverse_relay_counter().await;
    let (public_base_url, public_requests, public_task) = spawn_reverse_relay_counter().await;
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let assignment = reverse_assignment();
    let rendezvous = secondary_reverse_target(Some(mesh_base_url), public_base_url);
    let peer = primary_reverse_target(None, "http://127.0.0.1:1".to_string());
    let client =
        MeshAwareHttpClient::from_transport_clients(reqwest::Client::new(), reqwest::Client::new());
    client
        .set_reverse_route(
            peer.node_id.clone(),
            reverse_route(rendezvous, None, assignment),
        )
        .await;

    client
        .send_peer_reverse_request(&peer, reverse_request(), &ca.key_pem, &ca.cert_pem)
        .await
        .expect_err("counter response omits relay acknowledgements");

    assert_eq!(mesh_requests.load(Ordering::SeqCst), 1);
    assert_eq!(public_requests.load(Ordering::SeqCst), 0);
    mesh_task.abort();
    public_task.abort();
}

#[tokio::test]
async fn reverse_outer_request_uses_local_rendezvous_portal() {
    let (local_base_url, local_requests, local_task) = spawn_reverse_relay_counter().await;
    let (public_base_url, public_requests, public_task) = spawn_reverse_relay_counter().await;
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let assignment = reverse_assignment();
    let rendezvous = secondary_reverse_target(None, public_base_url);
    let peer = primary_reverse_target(None, "http://127.0.0.1:1".to_string());
    let client = MeshAwareHttpClient::new(reqwest::Client::new())
        .with_local_reverse_relay(xp_test_fixtures::secondary_node_id(), local_base_url);
    client
        .set_reverse_route(
            peer.node_id.clone(),
            reverse_route(rendezvous, None, assignment),
        )
        .await;

    client
        .send_peer_reverse_request(&peer, reverse_request(), &ca.key_pem, &ca.cert_pem)
        .await
        .expect_err("counter response omits relay acknowledgements");

    assert_eq!(local_requests.load(Ordering::SeqCst), 1);
    assert_eq!(public_requests.load(Ordering::SeqCst), 0);
    local_task.abort();
    public_task.abort();
}

#[tokio::test]
async fn reverse_health_probe_warms_primary_and_standby() {
    let (primary_base_url, primary_requests, primary_task) = spawn_reverse_relay_counter().await;
    let (standby_base_url, standby_requests, standby_task) = spawn_reverse_relay_counter().await;
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let mut assignment = reverse_assignment();
    assignment.standby_node_id = Some(xp_test_fixtures::tertiary_node_id().to_owned());
    let rendezvous = secondary_reverse_target(None, primary_base_url);
    let standby = tertiary_reverse_target(None, standby_base_url);
    let peer = primary_reverse_target(None, "http://127.0.0.1:1".to_string());
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    client
        .set_reverse_route(
            peer.node_id.clone(),
            reverse_route(rendezvous, Some(standby), assignment),
        )
        .await;

    client
        .send_peer_reverse_health_request(&peer, reverse_request(), &ca.key_pem, &ca.cert_pem)
        .await
        .expect_err("counter responses omit relay acknowledgements");

    assert_eq!(primary_requests.load(Ordering::SeqCst), 1);
    assert_eq!(standby_requests.load(Ordering::SeqCst), 1);
    primary_task.abort();
    standby_task.abort();
}
