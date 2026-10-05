use super::peer_target_tests::primary_reverse_target;
use super::*;
use crate::reconcile::ReconcileHandle;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Instant;
use tokio::sync::oneshot;

#[tokio::test]
async fn mesh_epoch_observation_serializes_concurrent_observers() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client =
        MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate, epoch.clone());
    let circuits = client.circuits();
    for _ in 0..MESH_FAILURES_BEFORE_OPEN {
        circuits.record_retryable_failure("peer").await;
    }
    epoch.store(1, Ordering::Release);
    let (first, second) = tokio::join!(client.observe_mesh_gate(), client.observe_mesh_gate());
    assert!(first && second);
    assert_eq!(circuits.state("peer", true).await, BreakerState::Open);
}

#[tokio::test]
async fn stale_mesh_epoch_cannot_reopen_current_breaker() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client =
        MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate, epoch.clone());
    let peer = primary_reverse_target(None, xp_test_fixtures::primary_api_url().to_string());
    let circuits = client.circuits();
    epoch.store(1, Ordering::Release);
    assert!(client.observe_mesh_gate().await);
    for _ in 0..MESH_FAILURES_BEFORE_OPEN {
        let gate_guard = client
            .mesh_direct_read_guard_until(Instant::now() + Duration::from_secs(1))
            .await
            .expect("mesh gate is enabled");
        client
            .record_mesh_transport_failure(
                &peer,
                MeshPeerReason::TransportError,
                "stale request".to_string(),
                0,
                gate_guard,
                None,
                Instant::now() + Duration::from_secs(1),
                None,
            )
            .await;
    }
    assert_eq!(circuits.state("peer", true).await, BreakerState::Closed);
}

#[tokio::test]
async fn mesh_attempt_is_rejected_after_gate_closes() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client =
        MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate.clone(), epoch);
    assert!(client.observe_mesh_gate().await);
    gate.store(false, Ordering::Release);
    assert!(!client.mesh_attempt_is_current(0).await);
}

#[tokio::test]
async fn normal_direct_preflight_obeys_closed_mesh_gate() {
    let gate = Arc::new(AtomicBool::new(false));
    let epoch = Arc::new(AtomicU64::new(0));
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate, epoch);
    let peer = primary_reverse_target(None, xp_test_fixtures::primary_api_url().to_string());
    let error = client
        .send_peer_direct_preflight(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: false,
                request_id: crate::id::new_ulid_string(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
                sender_id: xp_test_fixtures::primary_node_id().to_owned(),
                updates_active_path: false,
            },
            "unused",
            "unused",
        )
        .await
        .expect_err("normal health probes must not bypass a closed Mesh gate");
    assert!(matches!(
        error,
        MeshRequestError::InvalidTarget(message)
            if message == "Mesh is disabled by the cluster gate"
    ));
}

#[tokio::test]
async fn normal_direct_preflight_obeys_open_direct_circuit() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate, epoch);
    let peer = primary_reverse_target(None, xp_test_fixtures::primary_api_url().to_string());
    let circuits = client.circuits();
    for _ in 0..MESH_FAILURES_BEFORE_OPEN {
        circuits.record_retryable_failure(&peer.node_id).await;
    }
    let error = client
        .send_peer_direct_preflight(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: false,
                request_id: crate::id::new_ulid_string(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
                sender_id: xp_test_fixtures::primary_node_id().to_owned(),
                updates_active_path: false,
            },
            "unused",
            "unused",
        )
        .await
        .expect_err("normal health probes must honor the Direct circuit");
    assert!(matches!(
        error,
        MeshRequestError::CircuitOpen {
            path: "Direct Mesh",
            ..
        }
    ));
}

#[tokio::test]
async fn mesh_gate_transition_waits_for_an_inflight_send_boundary() {
    let reconcile = ReconcileHandle::noop();
    let gate = reconcile.mesh_gate();
    let epoch = reconcile.mesh_gate_epoch();
    let client = MeshAwareHttpClient::new(reqwest::Client::new())
        .with_mesh_gate_epoch(gate.clone(), epoch)
        .with_mesh_gate_lock(reconcile.mesh_gate_lock());
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();

    let send = tokio::spawn(async move {
        client
            .with_mesh_send_until(0, Instant::now() + Duration::from_secs(5), |_| async move {
                let _ = started_tx.send(());
                let _ = release_rx.await;
            })
            .await
    });
    started_rx.await.expect("send should enter the gate");

    let transition = tokio::spawn({
        let reconcile = reconcile.clone();
        async move { reconcile.initialize_mesh_gate(false).await }
    });
    tokio::task::yield_now().await;
    assert!(gate.load(Ordering::Acquire));

    let _ = release_tx.send(());
    let (_result, gate_guard) = send
        .await
        .expect("send task should finish")
        .expect("send should enter the gate");
    tokio::task::yield_now().await;
    assert!(gate.load(Ordering::Acquire));
    drop(gate_guard);
    transition.await.expect("transition task should finish");
    assert!(!gate.load(Ordering::Acquire));
}

#[tokio::test]
async fn mesh_admission_timeout_keeps_public_fallback_available_behind_a_queued_writer() {
    let reconcile = ReconcileHandle::noop();
    let gate = reconcile.mesh_gate();
    let epoch = reconcile.mesh_gate_epoch();
    let in_flight_mesh_read = reconcile.mesh_gate_lock().read_owned().await;
    let transition = tokio::spawn({
        let reconcile = reconcile.clone();
        async move { reconcile.initialize_mesh_gate(false).await }
    });
    tokio::task::yield_now().await;

    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, public_requests, public_task) =
        super::peer_target_tests::spawn_signed_public(&ca.key_pem, &ca.cert_pem).await;
    let (mesh_base_url, mesh_requests, mesh_task) =
        super::peer_target_tests::spawn_stalling_mesh().await;
    let peer = primary_reverse_target(Some(mesh_base_url), public_base_url);
    let client =
        MeshAwareHttpClient::from_transport_clients(reqwest::Client::new(), reqwest::Client::new())
            .with_mesh_gate_epoch(gate, epoch)
            .with_mesh_gate_lock(reconcile.mesh_gate_lock());

    let result = tokio::time::timeout(
        Duration::from_millis(900),
        client.send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: true,
                request_id: "mesh-admission-timeout-public-fallback".to_owned(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
                updates_active_path: true,
            },
            &ca.key_pem,
            &ca.cert_pem,
        ),
    )
    .await
    .expect("Mesh admission must not wait for the queued writer");
    assert!(
        result.is_ok(),
        "Public fallback should receive the request: {result:?}"
    );
    assert_eq!(mesh_requests.load(Ordering::SeqCst), 0);
    assert_eq!(public_requests.load(Ordering::SeqCst), 1);

    drop(in_flight_mesh_read);
    transition
        .await
        .expect("gate transition task should finish");
    mesh_task.abort();
    public_task.abort();
}

#[tokio::test]
async fn mesh_admission_timeout_releases_half_open_probe_before_public_fallback() {
    let reconcile = ReconcileHandle::noop();
    let gate = reconcile.mesh_gate();
    let epoch = reconcile.mesh_gate_epoch();
    let in_flight_mesh_read = reconcile.mesh_gate_lock().read_owned().await;
    let transition = tokio::spawn({
        let reconcile = reconcile.clone();
        async move { reconcile.initialize_mesh_gate(false).await }
    });
    tokio::task::yield_now().await;

    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, public_requests, public_task) =
        super::peer_target_tests::spawn_signed_public(&ca.key_pem, &ca.cert_pem).await;
    let (mesh_base_url, mesh_requests, mesh_task) =
        super::peer_target_tests::spawn_stalling_mesh().await;
    let peer = primary_reverse_target(Some(mesh_base_url), public_base_url);
    let client =
        MeshAwareHttpClient::from_transport_clients(reqwest::Client::new(), reqwest::Client::new())
            .with_mesh_gate_epoch(gate, epoch)
            .with_mesh_gate_lock(reconcile.mesh_gate_lock());
    {
        let circuits = client.circuits();
        let mut peers = circuits.peers.lock().await;
        let circuit = peers.entry(peer.node_id.clone()).or_default();
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }

    let result = client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: true,
                request_id: "mesh-admission-timeout-half-open".to_owned(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
                updates_active_path: true,
            },
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await
        .expect("Public fallback should receive the request");
    drop(result);

    assert_eq!(mesh_requests.load(Ordering::SeqCst), 0);
    assert_eq!(public_requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        client
            .before_mesh_request(&peer.node_id, true, InternalRoute::HealthV2)
            .await
            .0,
        MeshAttemptDecision::Probe
    );

    drop(in_flight_mesh_read);
    transition
        .await
        .expect("gate transition task should finish");
    mesh_task.abort();
    public_task.abort();
}

#[tokio::test]
async fn direct_mesh_admission_timeout_preserves_pre_dispatch_classification() {
    let reconcile = ReconcileHandle::noop();
    let gate = reconcile.mesh_gate();
    let epoch = reconcile.mesh_gate_epoch();
    let in_flight_mesh_read = reconcile.mesh_gate_lock().read_owned().await;
    let transition = tokio::spawn({
        let reconcile = reconcile.clone();
        async move { reconcile.initialize_mesh_gate(false).await }
    });
    tokio::task::yield_now().await;

    let client = MeshAwareHttpClient::new(reqwest::Client::new())
        .with_mesh_gate_epoch(gate, epoch)
        .with_mesh_gate_lock(reconcile.mesh_gate_lock());
    let peer = primary_reverse_target(
        Some("https://peer.example:443".to_owned()),
        "https://public.example".to_owned(),
    );
    let error = client
        .send_peer_direct_preflight(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_millis(50),
                allow_ambiguous_fallback: false,
                request_id: "direct-mesh-admission-timeout".to_owned(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
                sender_id: "sender".to_owned(),
                updates_active_path: false,
            },
            "cluster-ca-key",
            "cluster-ca-cert",
        )
        .await
        .expect_err("direct admission must fail before dispatch");
    assert!(matches!(error, MeshRequestError::PreDispatchTimeout));

    drop(in_flight_mesh_read);
    transition
        .await
        .expect("gate transition task should finish");
}

#[tokio::test]
async fn mesh_response_body_deadline_releases_gate_guard() {
    use futures_util::StreamExt;

    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::wrap_stream(futures_util::stream::pending::<
                Result<bytes::Bytes, std::io::Error>,
            >()))
            .expect("synthetic response"),
    );
    let response = super::reverse::attach_mesh_gate(
        response,
        gate_guard,
        std::time::Instant::now() + Duration::from_millis(40),
    );
    let mut body = response.bytes_stream();
    assert!(
        tokio::time::timeout(Duration::from_millis(200), body.next())
            .await
            .expect("body deadline should fire")
            .expect("deadline should produce one body error")
            .is_err()
    );
    drop(body);
    tokio::time::timeout(Duration::from_millis(100), gate_lock.write_owned())
        .await
        .expect("body deadline must release the gate guard");
}

#[tokio::test]
async fn mesh_response_body_guard_is_released_on_eof() {
    use futures_util::StreamExt;

    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::from("response-body"))
            .expect("synthetic response"),
    );
    let response = super::reverse::attach_mesh_gate(
        response,
        gate_guard,
        Instant::now() + Duration::from_secs(1),
    );
    let mut body = response.bytes_stream();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), gate_lock.clone().write_owned())
            .await
            .is_err(),
        "body guard must remain through response data"
    );
    assert_eq!(
        body.next()
            .await
            .expect("response data")
            .expect("response data is valid"),
        bytes::Bytes::from_static(b"response-body")
    );
    assert!(body.next().await.is_none());
    tokio::time::timeout(Duration::from_millis(100), gate_lock.write_owned())
        .await
        .expect("EOF must release the gate guard");
}

#[tokio::test]
async fn mesh_response_body_guard_is_released_on_error() {
    use futures_util::StreamExt;

    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::wrap_stream(futures_util::stream::iter([
                Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"partial")),
                Err(std::io::Error::other("synthetic body failure")),
            ])))
            .expect("synthetic response"),
    );
    let response = super::reverse::attach_mesh_gate(
        response,
        gate_guard,
        Instant::now() + Duration::from_secs(1),
    );
    let mut body = response.bytes_stream();
    assert_eq!(
        body.next()
            .await
            .expect("response data")
            .expect("first response data is valid"),
        bytes::Bytes::from_static(b"partial")
    );
    assert!(body.next().await.expect("response error").is_err());
    tokio::time::timeout(Duration::from_millis(100), gate_lock.write_owned())
        .await
        .expect("body error must release the gate guard");
}

#[tokio::test]
async fn mesh_response_body_guard_is_released_when_response_is_dropped() {
    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::wrap_stream(futures_util::stream::pending::<
                Result<bytes::Bytes, std::io::Error>,
            >()))
            .expect("synthetic response"),
    );
    let response = super::reverse::attach_mesh_gate(
        response,
        gate_guard,
        Instant::now() + Duration::from_secs(1),
    );
    drop(response);
    tokio::time::timeout(Duration::from_millis(100), gate_lock.write_owned())
        .await
        .expect("dropping the response must release the gate guard");
}

#[tokio::test]
async fn mesh_response_body_deadline_releases_unpolled_guard() {
    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::wrap_stream(futures_util::stream::pending::<
                Result<bytes::Bytes, std::io::Error>,
            >()))
            .expect("synthetic response"),
    );
    let response = super::reverse::attach_mesh_gate(
        response,
        gate_guard,
        Instant::now() + Duration::from_millis(40),
    );
    let body = response.bytes_stream();
    tokio::time::sleep(Duration::from_millis(100)).await;
    tokio::time::timeout(Duration::from_millis(100), gate_lock.write_owned())
        .await
        .expect("body deadline must release an unpolled guard");
    drop(body);
}

#[tokio::test]
async fn mesh_epoch_change_releases_half_open_probe_without_resetting_backoff() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client =
        MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate, epoch.clone());
    let circuits = client.circuits();
    {
        let mut peers = circuits.peers.lock().await;
        let circuit = peers.entry("peer".to_owned()).or_default();
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }
    assert_eq!(
        circuits.before_attempt("peer", true).await,
        MeshAttemptDecision::Probe
    );
    epoch.store(1, Ordering::Release);
    assert!(client.observe_mesh_gate().await);
    assert_eq!(
        circuits.before_attempt("peer", true).await,
        MeshAttemptDecision::Probe
    );
    assert_eq!(circuits.state("peer", true).await, BreakerState::HalfOpen);
}

#[tokio::test]
async fn stale_mesh_probe_releases_its_half_open_slot_before_public_fallback() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client =
        MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate, epoch.clone());
    {
        let circuits = client.circuits();
        let mut peers = circuits.peers.lock().await;
        let circuit = peers.entry("peer".to_owned()).or_default();
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }
    let (decision, stale_epoch) = client
        .before_mesh_request("peer", true, InternalRoute::HealthV2)
        .await;
    assert_eq!(decision, MeshAttemptDecision::Probe);
    epoch.store(stale_epoch + 1, Ordering::Release);
    assert!(!client.mesh_attempt_is_current(stale_epoch).await);
    client
        .release_half_open_probe_for_epoch("peer", stale_epoch)
        .await;
    assert_eq!(
        client
            .before_mesh_request("peer", true, InternalRoute::HealthV2)
            .await
            .0,
        MeshAttemptDecision::Probe
    );
}

#[tokio::test]
async fn public_circuit_isolates_after_one_failed_request() {
    let circuits = PeerCircuitBreakers::default();
    assert_eq!(
        circuits.before_public_attempt("peer").await,
        MeshAttemptDecision::Attempt
    );
    assert_eq!(
        circuits.record_public_failure("peer").await,
        BreakerState::Open
    );
    assert_eq!(
        circuits.before_public_attempt("peer").await,
        MeshAttemptDecision::SkipOpen
    );
    assert_eq!(circuits.public_state("peer").await, BreakerState::Open);
    assert_eq!(
        circuits.record_public_success("peer").await,
        BreakerState::Closed
    );
    assert_eq!(circuits.public_state("peer").await, BreakerState::Closed);
}

#[tokio::test]
async fn half_open_circuit_rejects_non_health_requests() {
    let circuits = PeerCircuitBreakers::default();
    {
        let mut peers = circuits.peers.lock().await;
        let circuit = peers.entry("peer".to_owned()).or_default();
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }
    assert_eq!(
        circuits
            .before_attempt_with_probe("peer", true, false)
            .await,
        MeshAttemptDecision::SkipOpen
    );
    assert_eq!(
        circuits.before_attempt_with_probe("peer", true, true).await,
        MeshAttemptDecision::Probe
    );
}

#[tokio::test]
async fn direct_protocol_failure_quarantines_without_public_fallback() {
    let circuits = PeerCircuitBreakers::default();
    {
        let mut peers = circuits.peers.lock().await;
        peers.entry("peer".to_owned()).or_default().failures = 1;
    }
    assert_eq!(
        circuits.record_protocol_failure("peer").await,
        BreakerState::Open
    );
    assert_eq!(circuits.peers.lock().await["peer"].failures, 1);
    assert_eq!(
        circuits
            .before_attempt_with_probe("peer", true, false)
            .await,
        MeshAttemptDecision::Quarantined
    );
    assert_eq!(circuits.record_success("peer").await, BreakerState::Closed);
    assert_eq!(
        circuits.before_attempt("peer", true).await,
        MeshAttemptDecision::Attempt
    );
}

#[tokio::test]
async fn quarantined_direct_peer_allows_only_health_revalidation() {
    let circuits = PeerCircuitBreakers::default();
    assert_eq!(
        circuits.record_protocol_failure("peer").await,
        BreakerState::Open
    );
    {
        let mut peers = circuits.peers.lock().await;
        peers
            .get_mut("peer")
            .expect("protocol failure creates circuit")
            .retry_at = Some(Instant::now() - Duration::from_secs(1));
    }
    assert_eq!(
        circuits
            .before_attempt_with_probe("peer", true, false)
            .await,
        MeshAttemptDecision::Quarantined
    );
    assert_eq!(
        circuits.before_attempt_with_probe("peer", true, true).await,
        MeshAttemptDecision::Probe
    );
}

#[tokio::test]
async fn stale_epoch_protocol_failure_does_not_quarantine_current_circuit() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate, epoch);
    let peer = primary_reverse_target(None, xp_test_fixtures::primary_api_url().to_string());
    let gate_guard = client
        .mesh_direct_read_guard_until(Instant::now() + Duration::from_secs(1))
        .await
        .expect("mesh gate is enabled");
    assert!(
        client
            .record_protocol_failure_for_epoch(
                &peer,
                1,
                Some("stale-membership".to_owned()),
                &gate_guard,
                Instant::now() + Duration::from_secs(1),
                None,
            )
            .await
            .is_none()
    );
    assert_eq!(
        client.circuits().state("peer", true).await,
        BreakerState::Closed
    );
}

#[tokio::test]
async fn stale_epoch_transport_failure_does_not_overwrite_current_validation() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client = MeshAwareHttpClient::new(reqwest::Client::new())
        .with_direct_validation_required()
        .with_mesh_gate_epoch(gate, epoch.clone());
    let peer = primary_reverse_target(None, xp_test_fixtures::primary_api_url().to_string());

    client.mark_direct_validation_success_at(&peer, None).await;
    epoch.store(1, Ordering::Release);
    let gate_guard = client
        .mesh_direct_read_guard_until(Instant::now() + Duration::from_secs(1))
        .await
        .expect("mesh gate is enabled");
    client
        .record_mesh_transport_failure(
            &peer,
            MeshPeerReason::TransportError,
            "stale request".to_owned(),
            0,
            gate_guard,
            None,
            Instant::now() + Duration::from_secs(1),
            None,
        )
        .await;

    assert_eq!(
        client.direct_validation_state_for(&peer).await,
        DirectValidationState::Verified
    );
}

#[tokio::test]
async fn direct_pre_dispatch_auth_failure_does_not_charge_peer() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_direct_validation_required();
    let peer = primary_reverse_target(
        Some("http://127.0.0.1:1".to_owned()),
        "https://public.example".to_owned(),
    );
    let error = client
        .send_peer_direct_preflight(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: false,
                request_id: "direct-pre-dispatch-auth".to_owned(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
                updates_active_path: false,
            },
            "invalid-key",
            "invalid-cert",
        )
        .await
        .expect_err("local signing failure should stop before Direct dispatch");

    assert!(matches!(error, MeshRequestError::PreDispatchAuth(_)));
    assert_eq!(
        client.circuits().state(&peer.node_id, true).await,
        BreakerState::Closed
    );
    assert_eq!(
        client.direct_validation_state_for(&peer).await,
        DirectValidationState::ConfiguredUnverified
    );
    assert_eq!(
        client.circuits().before_attempt(&peer.node_id, true).await,
        MeshAttemptDecision::Attempt
    );
}

#[tokio::test]
async fn public_pre_dispatch_auth_failure_does_not_charge_peer() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer = primary_reverse_target(None, "https://public.example".to_owned());
    let error = client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::POST,
                path_and_query: "/api/admin/_internal/raft/client-write".to_owned(),
                content_type: Some("application/json".to_owned()),
                body: br#"{"op":"set"}"#.to_vec(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: true,
                request_id: "public-pre-dispatch-auth".to_owned(),
                route: InternalRoute::MeshV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
                updates_active_path: true,
            },
            "invalid-key",
            "invalid-cert",
        )
        .await
        .expect_err("local signing failure should stop before Public dispatch");

    assert!(matches!(error, MeshRequestError::PreDispatchAuth(_)));
    assert_eq!(
        client.circuits().public_state(&peer.node_id).await,
        BreakerState::Closed
    );
    assert_eq!(
        client.circuits().before_public_attempt(&peer.node_id).await,
        MeshAttemptDecision::Attempt
    );
}

#[tokio::test]
async fn invalid_public_target_releases_half_open_probe() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let mut peer = primary_reverse_target(None, "not-a-url".to_owned());
    peer.mesh_base_url = None;
    client
        .circuits()
        .set_public_probe_ready_for_test(&peer.node_id)
        .await;

    let error = client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: true,
                request_id: "invalid-public-target-probe".to_owned(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::primary_node_id().to_owned(),
                updates_active_path: false,
            },
            "unused",
            "unused",
        )
        .await
        .expect_err("invalid public URL should fail before dispatch");
    assert!(matches!(error, MeshRequestError::InvalidTarget(_)));
    assert_eq!(
        client.circuits().before_public_attempt(&peer.node_id).await,
        MeshAttemptDecision::Probe
    );
}

#[tokio::test]
async fn cancelled_public_probe_releases_half_open_probe() {
    let (public_base_url, requests, public_task) =
        super::peer_target_tests::spawn_stalling_mesh().await;
    let peer = primary_reverse_target(None, public_base_url);
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    client
        .circuits()
        .set_public_probe_ready_for_test(&peer.node_id)
        .await;

    let request = tokio::spawn({
        let client = client.clone();
        let peer = peer.clone();
        let ca_key_pem = ca.key_pem.clone();
        let ca_cert_pem = ca.cert_pem.clone();
        async move {
            client
                .send_peer_request(
                    &peer,
                    MeshRequest {
                        method: reqwest::Method::GET,
                        path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
                        content_type: None,
                        body: Vec::new(),
                        total_budget: Duration::from_secs(5),
                        allow_ambiguous_fallback: true,
                        request_id: "cancelled-public-probe".to_owned(),
                        route: InternalRoute::HealthV2,
                        cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                        sender_id: xp_test_fixtures::primary_node_id().to_owned(),
                        updates_active_path: false,
                    },
                    &ca_key_pem,
                    &ca_cert_pem,
                )
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while requests.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("public probe should dispatch before cancellation");
    request.abort();
    let _ = request.await;

    let decision = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let decision = client.circuits().before_public_attempt(&peer.node_id).await;
            if decision == MeshAttemptDecision::Probe {
                break decision;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled public probe must release its half-open slot");
    assert_eq!(decision, MeshAttemptDecision::Probe);
    public_task.abort();
}

#[tokio::test]
async fn invalid_mesh_target_releases_half_open_probe() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer = primary_reverse_target(
        Some("not-a-url".to_owned()),
        "https://public.example".to_owned(),
    );
    let circuits = client.circuits();
    {
        let mut peers = circuits.peers.lock().await;
        let circuit = peers.entry(peer.node_id.clone()).or_default();
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }

    let error = client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: true,
                request_id: "invalid-mesh-target-probe".to_owned(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::primary_node_id().to_owned(),
                updates_active_path: false,
            },
            "unused",
            "unused",
        )
        .await
        .expect_err("invalid Mesh URL should fail before dispatch");
    assert!(matches!(error, MeshRequestError::InvalidTarget(_)));
    assert_eq!(
        circuits.before_attempt(&peer.node_id, true).await,
        MeshAttemptDecision::Probe
    );
}
