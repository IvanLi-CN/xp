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
        client
            .record_mesh_transport_failure(
                &peer,
                MeshPeerReason::TransportError,
                "stale request".to_string(),
                0,
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
            .with_mesh_send(0, || async move {
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
        .mesh_direct_read_guard()
        .await
        .expect("mesh gate is enabled");
    assert!(
        client
            .record_protocol_failure_for_epoch(
                &peer,
                1,
                Some("stale-membership".to_owned()),
                &gate_guard,
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
    client
        .mark_direct_validation_failure_for_epoch(
            &peer,
            DirectValidationState::TransportFailed,
            None,
            0,
        )
        .await;

    assert_eq!(
        client.direct_validation_state_for(&peer).await,
        DirectValidationState::Verified
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
