use super::super::peer_target_tests::{primary_reverse_target, spawn_signed_public_with_delay};
use super::super::*;

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
