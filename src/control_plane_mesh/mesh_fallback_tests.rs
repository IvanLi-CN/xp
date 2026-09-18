use super::peer_target_tests::{
    primary_reverse_target, reverse_assignment, reverse_route, secondary_reverse_target,
    spawn_reverse_relay_counter, spawn_signed_public,
};
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

#[tokio::test]
async fn capability_probe_rechecks_public_fallback_after_gate_closes() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, public_requests, public_task) =
        spawn_signed_public(&ca.key_pem, &ca.cert_pem).await;
    let peer = primary_reverse_target(None, public_base_url);
    let gate = Arc::new(AtomicBool::new(true));
    let observed = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let client =
        MeshAwareHttpClient::from_transport_clients(reqwest::Client::new(), reqwest::Client::new())
            .with_mesh_gate(gate.clone())
            .with_mesh_observation_pause(observed.clone(), release.clone());
    let request = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .send_peer_request_allowing_legacy_not_found(
                    &peer,
                    MeshRequest {
                        method: reqwest::Method::GET,
                        path_and_query: LEGACY_CAPABILITIES_PROBE_PATH.to_string(),
                        content_type: None,
                        body: Vec::new(),
                        total_budget: Duration::from_secs(1),
                        allow_ambiguous_fallback: false,
                        request_id: "capability-gate-close-race".to_string(),
                        route: InternalRoute::MeshV2,
                        cluster_id: xp_test_fixtures::cluster_fixture53().to_string(),
                        sender_id: xp_test_fixtures::primary_node_id().to_string(),
                        updates_active_path: true,
                    },
                    &ca.key_pem,
                    &ca.cert_pem,
                )
                .await
        }
    });
    observed.notified().await;
    gate.store(false, Ordering::Release);
    release.notify_one();

    let result = request.await.expect("capability probe task should finish");
    assert!(result.is_ok(), "public fallback should recover the probe");
    assert_eq!(public_requests.load(Ordering::SeqCst), 1);
    public_task.abort();
}

#[tokio::test]
async fn non_mesh_capability_probe_bypasses_reverse_assignment() {
    let (reverse_base_url, reverse_requests, reverse_task) = spawn_reverse_relay_counter().await;
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, public_requests, public_task) =
        spawn_signed_public(&ca.key_pem, &ca.cert_pem).await;
    let node = Node {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        access_host: xp_test_fixtures::primary_host().to_owned(),
        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: Default::default(),
    };
    let mut peer = peer_target_from_node(&node, &[]);
    peer.public_base_url = public_base_url;
    let rendezvous = secondary_reverse_target(None, reverse_base_url);
    let client =
        MeshAwareHttpClient::from_transport_clients(reqwest::Client::new(), reqwest::Client::new());
    client
        .set_reverse_route(
            peer.node_id.clone(),
            reverse_route(rendezvous, None, reverse_assignment()),
        )
        .await;

    let result = client
        .send_peer_request_allowing_legacy_not_found(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/capabilities".to_string(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: false,
                request_id: "non-mesh-capability-public-only".to_string(),
                route: InternalRoute::MeshV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_string(),
                sender_id: xp_test_fixtures::primary_node_id().to_string(),
                updates_active_path: true,
            },
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await;

    assert!(result.is_ok(), "public capability probe should succeed");
    assert_eq!(reverse_requests.load(Ordering::SeqCst), 0);
    assert_eq!(public_requests.load(Ordering::SeqCst), 1);
    reverse_task.abort();
    public_task.abort();
}
