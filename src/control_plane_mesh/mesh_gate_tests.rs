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
