use super::*;
use futures_util::StreamExt;

fn peer() -> MeshPeerTarget {
    MeshPeerTarget {
        node_id: "peer".to_owned(),
        node_name: "peer".to_owned(),
        mesh_base_url: Some("https://mesh.example".to_owned()),
        endpoint_transport: Some("xhttp_reality_fallback"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: "https://public.example".to_owned(),
    }
}

fn request() -> MeshRequest {
    MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(1),
        allow_ambiguous_fallback: false,
        request_id: "mesh-success-race-regression".to_owned(),
        route: InternalRoute::HealthV2,
        cluster_id: "cluster".to_owned(),
        sender_id: "sender".to_owned(),
        updates_active_path: false,
    }
}

fn transport() -> MeshTransportObservation {
    MeshTransportObservation {
        protocol: MeshTransportProtocol::H2,
        fingerprint: None,
    }
}

#[tokio::test]
async fn delayed_mesh_success_cannot_overwrite_newer_protocol_rejection() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer = peer();
    let operation_id = client.circuits.next_operation();
    let callback = client.mesh_success_telemetry_callback(
        &peer,
        Instant::now(),
        &request(),
        transport(),
        0,
        None,
        operation_id,
        None,
        Instant::now() + Duration::from_secs(1),
    );

    let newer_operation_id = client.circuits.next_operation();
    client
        .circuits
        .record_protocol_failure_at(&peer.node_id, newer_operation_id)
        .await
        .expect("newer protocol rejection should apply");
    callback(crate::mesh_gate_body::BodyFinish::Complete);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client
                .circuits
                .before_attempt_with_probe(&peer.node_id, true, false)
                .await
                == MeshAttemptDecision::Quarantined
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("newer protocol rejection must remain authoritative");
}

#[tokio::test]
async fn reenable_preflight_success_commits_while_mesh_gate_is_disabled() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer = peer();
    client.cluster_mesh_enabled.store(false, Ordering::Release);
    client.circuits.record_protocol_failure(&peer.node_id).await;
    client
        .mark_direct_validation_failure_at(&peer, DirectValidationState::ProtocolRejected, None)
        .await;

    client
        .record_direct_preflight_success_after_body(
            &peer,
            0,
            None,
            client.circuits.next_operation(),
            None,
            true,
            Instant::now() + Duration::from_secs(1),
        )
        .await;

    assert_eq!(
        client.circuits.state(&peer.node_id, true).await,
        BreakerState::Closed
    );
    assert_eq!(
        client.direct_validation_state_for(&peer).await,
        DirectValidationState::Verified
    );
}

#[tokio::test]
async fn mesh_probe_remains_held_until_response_body_finishes() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer = peer();
    let circuits = client.circuits();
    {
        let mut peers = circuits.peers.lock().await;
        let circuit = peers.entry(peer.node_id.clone()).or_default();
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }
    let (decision, probe_id) = circuits
        .before_attempt_with_probe_at_epoch_until_with_token(
            &peer.node_id,
            true,
            true,
            Some(0),
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .expect("probe admission");
    let probe_guard = client
        .mesh_probe_guard(&peer.node_id, decision, 0, probe_id)
        .expect("mesh probe guard");
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::from("response-body"))
            .expect("synthetic response"),
    );
    let response = super::reverse::attach_mesh_gate_with_finish(
        response,
        client.mesh_gate_lock.clone().read_owned().await,
        Instant::now() + Duration::from_secs(1),
        Some(client.mesh_success_telemetry_callback(
            &peer,
            Instant::now(),
            &request(),
            transport(),
            0,
            None,
            circuits.next_operation(),
            Some(probe_guard),
            Instant::now() + Duration::from_secs(1),
        )),
    );
    let mut body = response.bytes_stream();
    assert_eq!(
        circuits
            .before_attempt_with_probe(&peer.node_id, true, true)
            .await,
        MeshAttemptDecision::SkipOpen,
        "an active response body must retain the half-open slot"
    );
    assert!(body.next().await.expect("response data").is_ok());
    assert!(body.next().await.is_none());

    let decision = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let decision = circuits
                .before_attempt_with_probe_at_epoch_until_with_token(
                    &peer.node_id,
                    true,
                    true,
                    Some(0),
                    Instant::now() + Duration::from_millis(100),
                )
                .await
                .expect("attempt admission")
                .0;
            if decision == MeshAttemptDecision::Attempt {
                break decision;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("body completion must close the successful circuit");
    assert_eq!(decision, MeshAttemptDecision::Attempt);
}
