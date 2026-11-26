use super::super::peer_target_tests::{primary_reverse_target, spawn_signed_public_with_delay};
use super::super::*;
use crate::mesh_telemetry::PublicFallbackHealth;

#[test]
fn public_direct_api_url_rejects_plain_http() {
    let error = join_url("http://peer.example", "/api/health", true)
        .expect_err("public direct requests must require HTTPS");
    assert!(matches!(error, MeshRequestError::InvalidTarget(_)));
}

#[tokio::test]
async fn public_health_ack_after_five_seconds_closes_the_expired_circuit() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, _, server) =
        spawn_signed_public_with_delay(&ca.key_pem, &ca.cert_pem, Duration::from_secs(6)).await;
    let peer = primary_reverse_target(None, public_base_url);
    let client = MeshAwareHttpClient::from_transport_clients(
        reqwest::Client::new(),
        reqwest::Client::builder().http1_only().build().unwrap(),
    )
    .with_mesh_gate_epoch(
        Arc::new(AtomicBool::new(false)),
        Arc::new(std::sync::atomic::AtomicU64::new(0)),
    );
    let circuits = client.circuits();
    circuits.record_public_failure(&peer.node_id).await;
    circuits
        .set_public_probe_ready_for_test(&peer.node_id)
        .await;
    let response = client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".into(),
                content_type: None,
                body: Vec::new(),
                total_budget: PUBLIC_HEALTH_PROBE_BUDGET,
                allow_ambiguous_fallback: false,
                request_id: crate::id::new_ulid_string(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().into(),
                sender_id: xp_test_fixtures::tertiary_node_id().into(),
                updates_active_path: false,
            },
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await
        .expect("a valid public ACK must recover the circuit after five seconds");
    response
        .bytes()
        .await
        .expect("consume signed health response");
    tokio::time::timeout(Duration::from_secs(1), async {
        while circuits.public_state(&peer.node_id).await != BreakerState::Closed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("verified body completion must close the expired public circuit");
    server.abort();
}

#[tokio::test]
async fn verified_public_probe_body_closes_the_open_circuit() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, _, server) =
        spawn_signed_public_with_delay(&ca.key_pem, &ca.cert_pem, Duration::ZERO).await;
    let peer = primary_reverse_target(None, public_base_url);
    let client = MeshAwareHttpClient::from_transport_clients(
        reqwest::Client::new(),
        reqwest::Client::builder().http1_only().build().unwrap(),
    );
    let circuits = client.circuits();
    circuits.record_public_failure(&peer.node_id).await;
    assert_eq!(
        circuits.public_state(&peer.node_id).await,
        BreakerState::Open
    );

    let response = client
        .send_public_probe(
            &peer,
            &MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".into(),
                content_type: None,
                body: Vec::new(),
                total_budget: PUBLIC_HEALTH_PROBE_BUDGET,
                allow_ambiguous_fallback: false,
                request_id: crate::id::new_ulid_string(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().into(),
                sender_id: xp_test_fixtures::tertiary_node_id().into(),
                updates_active_path: false,
            },
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await
        .expect("public health probe must verify the signed headers");
    response
        .bytes()
        .await
        .expect("consume the signed public health body");
    assert!(client.record_public_health_success(&peer.node_id).await);
    assert_eq!(
        circuits.public_state(&peer.node_id).await,
        BreakerState::Closed
    );
    server.abort();
}

#[tokio::test]
async fn public_probe_recovery_closes_only_the_circuit_and_emits_one_event() {
    let telemetry_dir = tempfile::tempdir().expect("telemetry directory");
    let telemetry = MeshTelemetryHandle::load(telemetry_dir.path()).expect("telemetry");
    let client =
        MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_observability(telemetry.clone());
    let peer_id = xp_test_fixtures::primary_node_id();
    let name = xp_test_fixtures::primary_node_name();
    let circuits = client.circuits();
    circuits.record_public_failure(peer_id).await;

    telemetry
        .set_public_fallback_health(peer_id, name, PublicFallbackHealth::Available)
        .await
        .expect("record public health");
    assert!(client.record_public_health_success(peer_id).await);
    assert!(client.record_public_health_success(peer_id).await);

    let snapshot = telemetry.snapshot().await;
    let peer = snapshot
        .peers
        .iter()
        .find(|item| item.peer_id == peer_id)
        .expect("peer telemetry");
    assert_eq!(peer.public_breaker, Some(BreakerState::Closed));
    assert_eq!(peer.public_fallback_health, PublicFallbackHealth::Available);
    assert!(
        peer.buckets.is_empty(),
        "probe recovery must not write functional buckets"
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.kind == "public_circuit")
            .count(),
        1,
        "only the transition should emit an event"
    );
}
