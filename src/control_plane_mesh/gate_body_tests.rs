use super::*;

#[tokio::test]
async fn mesh_body_error_does_not_commit_success_state() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_direct_validation_required();
    let peer = MeshPeerTarget {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
        endpoint_transport: Some("xhttp_reality_fallback"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
    };
    client
        .circuits
        .record_retryable_failure_at(
            xp_test_fixtures::primary_node_id(),
            client.circuits.next_operation(),
        )
        .await;
    client
        .mark_direct_validation_failure_at(&peer, DirectValidationState::TransportFailed, None)
        .await;
    let request = MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(1),
        allow_ambiguous_fallback: false,
        request_id: "body-error-state-regression".to_owned(),
        route: InternalRoute::HealthV2,
        cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
        sender_id: "sender".to_owned(),
        updates_active_path: false,
    };
    let callback = || {
        client.mesh_success_telemetry_callback(
            &peer,
            Instant::now(),
            &request,
            MeshTransportObservation {
                protocol: MeshTransportProtocol::H2,
                fingerprint: None,
            },
            0,
            None,
            client.circuits.next_operation(),
            None,
            Instant::now() + Duration::from_secs(1),
        )
    };
    callback()(crate::mesh_gate_body::BodyFinish::Error);
    tokio::task::yield_now().await;
    assert_eq!(
        client.direct_validation_state_for(&peer).await,
        DirectValidationState::TransportFailed
    );
    assert_eq!(
        client
            .circuits
            .peers
            .lock()
            .await
            .get(xp_test_fixtures::primary_node_id())
            .expect("failed peer circuit")
            .failures,
        2
    );
    callback()(crate::mesh_gate_body::BodyFinish::Complete);
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client.direct_validation_state_for(&peer).await == DirectValidationState::Verified
                && client
                    .circuits
                    .peers
                    .lock()
                    .await
                    .get(xp_test_fixtures::primary_node_id())
                    .is_some_and(|circuit| circuit.failures == 0)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("complete body must commit success state without telemetry");
}

#[tokio::test]
async fn mesh_success_telemetry_skips_after_epoch_changes() {
    let temp = tempfile::tempdir().expect("telemetry directory");
    let telemetry = MeshTelemetryHandle::load(temp.path()).expect("telemetry");
    let client =
        MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_observability(telemetry.clone());
    let peer = MeshPeerTarget {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
        endpoint_transport: Some("xhttp_reality_fallback"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
    };
    let barrier_writer = client.mesh_epoch_barrier.clone().write_owned().await;
    let task = tokio::spawn({
        let client = client.clone();
        let peer = peer.clone();
        async move {
            client
                .record_mesh_success_after_body(
                    &peer,
                    Instant::now(),
                    false,
                    MeshTransportObservation {
                        protocol: MeshTransportProtocol::H2,
                        fingerprint: None,
                    },
                    0,
                    None,
                    client.circuits.next_operation(),
                    None,
                    Instant::now() + Duration::from_secs(1),
                )
                .await;
        }
    });
    tokio::task::yield_now().await;
    client.cluster_mesh_epoch.store(1, Ordering::Release);
    drop(barrier_writer);
    task.await.expect("telemetry task should finish");
    assert!(telemetry.snapshot().await.peers.is_empty());
}
