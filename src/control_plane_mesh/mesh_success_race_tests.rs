use super::*;
use futures_util::StreamExt;

fn peer() -> MeshPeerTarget {
    MeshPeerTarget {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
        endpoint_transport: Some("xhttp_reality_fallback"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
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
        cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
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
    let (completion_tx, completion_rx) = tokio::sync::oneshot::channel();
    let callback = client.mesh_success_telemetry_callback_with_completion(
        &peer,
        Instant::now(),
        &request(),
        transport(),
        0,
        None,
        operation_id,
        None,
        Instant::now() + Duration::from_secs(1),
        completion_tx,
    );

    let newer_operation_id = client.circuits.next_operation();
    client
        .circuits
        .record_protocol_failure_at(&peer.node_id, newer_operation_id)
        .await
        .expect("newer protocol rejection should apply");
    callback(crate::mesh_gate_body::BodyFinish::Complete);
    tokio::time::timeout(Duration::from_secs(1), completion_rx)
        .await
        .expect("queued Mesh body completion should run")
        .expect("Mesh body completion signal");
    assert_eq!(
        client
            .circuits
            .before_attempt_with_probe(&peer.node_id, true, false)
            .await,
        MeshAttemptDecision::Quarantined,
        "newer protocol rejection must remain authoritative after the old body task runs"
    );
}

#[tokio::test]
async fn older_mesh_body_completion_cannot_evict_newer_state_when_queue_is_full() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer = peer();
    for _ in 0..2 {
        let operation_id = client.circuits.next_operation();
        client
            .circuits
            .record_retryable_failure_at(&peer.node_id, operation_id)
            .await;
    }

    let release = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
    let active = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for index in 0..super::completion::COMPLETION_ACTIVE_CAPACITY {
        let release = release.clone();
        let active = active.clone();
        client.dispatch_critical_completion(format!("active-{index}"), async move {
            active.fetch_add(1, Ordering::AcqRel);
            release
                .acquire()
                .await
                .expect("active completion release")
                .forget();
        });
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while active.load(Ordering::Acquire) != super::completion::COMPLETION_ACTIVE_CAPACITY {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("completion worker should fill its active window");

    for index in 0..super::completion::COMPLETION_QUEUE_CAPACITY {
        let release = release.clone();
        client.dispatch_critical_completion(format!("queued-{index}"), async move {
            release
                .acquire()
                .await
                .expect("queued completion release")
                .forget();
        });
    }

    let old_operation = client.circuits.next_operation();
    let new_operation = client.circuits.next_operation();
    let (new_tx, new_rx) = tokio::sync::oneshot::channel();
    let new_completion = client.mesh_success_telemetry_callback_with_completion(
        &peer,
        Instant::now(),
        &request(),
        transport(),
        0,
        None,
        new_operation,
        None,
        Instant::now() + Duration::from_secs(1),
        new_tx,
    );
    let (old_tx, old_rx) = tokio::sync::oneshot::channel();
    let old_completion = client.mesh_success_telemetry_callback_with_completion(
        &peer,
        Instant::now(),
        &request(),
        transport(),
        0,
        None,
        old_operation,
        None,
        Instant::now() + Duration::from_secs(1),
        old_tx,
    );
    new_completion(crate::mesh_gate_body::BodyFinish::Deadline);
    old_completion(crate::mesh_gate_body::BodyFinish::Complete);

    release.add_permits(
        super::completion::COMPLETION_ACTIVE_CAPACITY
            + super::completion::COMPLETION_QUEUE_CAPACITY,
    );
    tokio::time::timeout(Duration::from_secs(2), new_rx)
        .await
        .expect("newer completion should run after queue recovery")
        .expect("newer completion must not be evicted by an older event");
    let _ = tokio::time::timeout(Duration::from_secs(2), old_rx)
        .await
        .expect("older completion should either run or be superseded");

    assert_eq!(
        client
            .circuits
            .before_attempt_with_probe(&peer.node_id, true, false)
            .await,
        MeshAttemptDecision::SkipOpen,
        "the newest transport failure must remain after all dispatched work completes"
    );
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
