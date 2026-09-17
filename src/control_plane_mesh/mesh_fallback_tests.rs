use super::peer_target_tests::{primary_reverse_target, spawn_signed_public};
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
