use super::*;

#[tokio::test]
async fn disarmed_mesh_probe_guard_cannot_release_a_new_probe() {
    let circuits = PeerCircuitBreakers::default();
    {
        let mut peers = circuits.peers.lock().await;
        let circuit = peers.entry("peer".to_owned()).or_default();
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }
    let decision = circuits
        .before_attempt_with_probe_at_epoch("peer", true, true, Some(7))
        .await;
    let old_probe_id = circuits
        .peers
        .lock()
        .await
        .get("peer")
        .and_then(|circuit| circuit.half_open_probe_id)
        .expect("first health request should own the probe slot");
    let old_guard = MeshHalfOpenProbeGuard::new(&circuits, "peer", decision, 7, Some(old_probe_id))
        .expect("first health request should own the probe slot");
    circuits
        .release_half_open_probe_for_epoch("peer", 7, old_probe_id)
        .await;

    let next_decision = circuits
        .before_attempt_with_probe_at_epoch("peer", true, true, Some(7))
        .await;
    assert_eq!(next_decision, MeshAttemptDecision::Probe);
    let next_probe_id = circuits
        .peers
        .lock()
        .await
        .get("peer")
        .and_then(|circuit| circuit.half_open_probe_id)
        .expect("second health request should own the probe slot");
    let _next_guard =
        MeshHalfOpenProbeGuard::new(&circuits, "peer", next_decision, 7, Some(next_probe_id));
    let (old_guard, drop_completed) = old_guard.with_drop_completion_for_test();
    drop(old_guard);
    tokio::time::timeout(Duration::from_secs(1), drop_completed)
        .await
        .expect("probe guard drop task should finish")
        .expect("probe guard drop completion");
    assert_eq!(
        circuits.before_attempt("peer", true).await,
        MeshAttemptDecision::SkipOpen
    );
}

#[tokio::test]
async fn deferred_direct_success_cannot_clear_a_new_half_open_probe() {
    let circuits = PeerCircuitBreakers::default();
    {
        let mut peers = circuits.peers.lock().await;
        let circuit = peers.entry("peer".to_owned()).or_default();
        circuit.failures = MESH_FAILURES_BEFORE_OPEN;
        circuit.retry_at = Some(Instant::now() - Duration::from_secs(1));
    }
    let (old_decision, old_probe_id) = circuits
        .before_attempt_with_probe_at_epoch_with_token("peer", true, true, Some(7))
        .await;
    let old_probe_id = old_probe_id.expect("old probe token");
    assert_eq!(old_decision, MeshAttemptDecision::Probe);
    circuits
        .release_half_open_probe_for_epoch("peer", 7, old_probe_id)
        .await;

    let (new_decision, new_probe_id) = circuits
        .before_attempt_with_probe_at_epoch_with_token("peer", true, true, Some(7))
        .await;
    let new_probe_id = new_probe_id.expect("new probe token");
    assert_eq!(new_decision, MeshAttemptDecision::Probe);
    let result = circuits
        .record_success_at_with_cleanup(
            "peer",
            circuits.next_operation(),
            Some(cleanup::DirectCleanupContext::new(
                Instant::now() + Duration::from_secs(1),
                Some(old_probe_id),
            )),
        )
        .await;

    assert_eq!(result, None, "stale cleanup must not mutate the circuit");
    assert_eq!(
        circuits
            .peers
            .lock()
            .await
            .get("peer")
            .and_then(|circuit| circuit.half_open_probe_id),
        Some(new_probe_id),
        "the new probe token must remain owned by the current request"
    );
}

#[tokio::test]
async fn direct_cleanup_does_not_write_after_its_deadline_while_waiting_for_lock() {
    let circuits = PeerCircuitBreakers::default();
    {
        let mut peers = circuits.peers.lock().await;
        peers.entry("peer".to_owned()).or_default().failures = 1;
    }
    let peers_lock = circuits.peers.clone().lock_owned().await;
    let deadline = Instant::now() + Duration::from_millis(20);
    let operation_id = circuits.next_operation();
    let cleanup_circuits = circuits.clone();
    let cleanup = tokio::spawn(async move {
        cleanup_circuits
            .record_success_at_with_cleanup(
                "peer",
                operation_id,
                Some(cleanup::DirectCleanupContext::new(deadline, None)),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    drop(peers_lock);

    assert_eq!(cleanup.await.expect("cleanup task"), None);
    assert_eq!(
        circuits
            .peers
            .lock()
            .await
            .get("peer")
            .map(|circuit| circuit.failures),
        Some(1),
        "expired cleanup must leave the circuit unchanged after acquiring its lock"
    );
}
