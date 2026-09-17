use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::time::sleep;

#[tokio::test]
async fn mesh_epoch_reset_serializes_concurrent_observers() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client =
        MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate, epoch.clone());
    let circuits = client.circuits();
    for _ in 0..MESH_FAILURES_BEFORE_OPEN {
        circuits.record_retryable_failure("peer").await;
    }
    epoch.store(1, Ordering::Release);
    let peers_guard = circuits.peers.lock().await;
    let first = tokio::spawn({
        let client = client.clone();
        async move { client.observe_mesh_gate().await }
    });
    let second = tokio::spawn({
        let client = client.clone();
        async move { client.observe_mesh_gate().await }
    });
    sleep(Duration::from_millis(10)).await;
    assert!(!first.is_finished() && !second.is_finished());
    drop(peers_guard);
    assert!(first.await.expect("first observer join"));
    assert!(second.await.expect("second observer join"));
    assert_eq!(circuits.state("peer", true).await, BreakerState::Closed);
}

#[tokio::test]
async fn stale_mesh_epoch_cannot_reopen_current_breaker() {
    let gate = Arc::new(AtomicBool::new(true));
    let epoch = Arc::new(AtomicU64::new(0));
    let client =
        MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_gate_epoch(gate, epoch.clone());
    let peer = MeshPeerTarget {
        node_id: "peer".to_string(),
        node_name: "peer".to_string(),
        mesh_base_url: None,
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: "https://peer.example".to_string(),
    };
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
