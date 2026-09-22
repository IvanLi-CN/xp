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
    assert_eq!(
        circuits.record_protocol_failure("peer").await,
        BreakerState::Open
    );
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
    assert!(
        client
            .record_protocol_failure_for_epoch(&peer, 1, Some("stale-membership".to_owned()),)
            .await
            .is_none()
    );
    assert_eq!(
        client.circuits().state("peer", true).await,
        BreakerState::Closed
    );
}
