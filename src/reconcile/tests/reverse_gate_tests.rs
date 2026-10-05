use super::*;

#[test]
fn reverse_gate_requires_a_runtime_reconcile_after_xray_availability_is_lost() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let reconcile = ReconcileHandle::from_sender(tx);
    assert!(reconcile.reverse_gate().load(Ordering::Acquire));

    reconcile.set_reverse_enabled(false);
    assert!(!reconcile.reverse_gate().load(Ordering::Acquire));

    reconcile.set_reverse_enabled(true);
    assert!(!reconcile.reverse_gate().load(Ordering::Acquire));

    reconcile.set_reverse_runtime_ready(true);
    assert!(reconcile.reverse_gate().load(Ordering::Acquire));
}

#[tokio::test]
async fn mesh_gate_can_be_initialized_from_persisted_cluster_state() {
    let reconcile = ReconcileHandle::noop();
    assert!(reconcile.mesh_gate().load(Ordering::Acquire));

    reconcile.initialize_mesh_gate(false).await;
    assert!(!reconcile.mesh_gate().load(Ordering::Acquire));

    reconcile.initialize_mesh_gate(true).await;
    assert!(reconcile.mesh_gate().load(Ordering::Acquire));
}

#[tokio::test]
async fn mesh_gate_only_resets_reverse_readiness_on_enable_transition() {
    let reconcile = ReconcileHandle::noop();
    reconcile.set_reverse_runtime_ready(false);

    reconcile.initialize_mesh_gate(true).await;
    assert!(!reconcile.reverse_gate().load(Ordering::Acquire));

    reconcile.set_reverse_runtime_ready(true);
    assert!(reconcile.reverse_gate().load(Ordering::Acquire));

    reconcile.initialize_mesh_gate(false).await;
    reconcile.initialize_mesh_gate(true).await;
    assert!(!reconcile.reverse_gate().load(Ordering::Acquire));
}

#[tokio::test]
async fn unchanged_authoritative_reconcile_does_not_queue_gate_writer() {
    let reconcile = ReconcileHandle::noop();
    let in_flight_mesh_read = reconcile.mesh_gate_lock().read_owned().await;
    reconcile.note_mesh_state_applied();
    let reconcile_task = tokio::spawn({
        let reconcile = reconcile.clone();
        async move {
            reconcile.set_mesh_enabled_if_current(true, 0).await;
        }
    });
    tokio::task::yield_now().await;

    let later_reader = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        reconcile.mesh_gate_lock().read_owned(),
    )
    .await
    .expect("unchanged reconcile must not queue a gate writer");
    drop(later_reader);
    drop(in_flight_mesh_read);
    reconcile_task
        .await
        .expect("reconcile task should not panic");
}

#[tokio::test]
async fn stale_mesh_disable_timeout_cannot_close_a_newer_generation() {
    let reconcile = ReconcileHandle::noop();
    let generation = reconcile.mesh_state_generation.load(Ordering::Acquire);
    let in_flight_mesh_read = reconcile.mesh_gate_lock().read_owned().await;
    let stale_reconcile = tokio::spawn({
        let reconcile = reconcile.clone();
        async move {
            reconcile
                .set_mesh_enabled_if_current(false, generation)
                .await;
        }
    });
    tokio::task::yield_now().await;

    reconcile.note_mesh_state_applied();
    assert!(reconcile.mesh_gate().load(Ordering::Acquire));
    stale_reconcile
        .await
        .expect("stale reconcile task should finish after its deadline");
    drop(in_flight_mesh_read);

    assert!(
        reconcile.mesh_gate().load(Ordering::Acquire),
        "a timed-out old disable must not override the newer authenticated generation"
    );
}

#[tokio::test]
async fn snapshot_hold_rejects_reconcile_until_authenticated_state_applies() {
    let reconcile = ReconcileHandle::noop();
    let generation = reconcile.mesh_state_generation.load(Ordering::Acquire);
    reconcile.hold_mesh_gate_until_raft_state().await;

    reconcile
        .set_mesh_enabled_if_current(true, generation)
        .await;
    assert!(
        !reconcile.mesh_gate().load(Ordering::Acquire),
        "a stale reconcile cannot reopen Mesh without authenticated snapshot/state evidence"
    );
    reconcile
        .set_mesh_enabled_if_current(
            true,
            reconcile.mesh_state_generation.load(Ordering::Acquire),
        )
        .await;
    assert!(
        !reconcile.mesh_gate().load(Ordering::Acquire),
        "a fresh reconcile of unauthenticated state must also remain closed"
    );

    reconcile.note_mesh_state_applied();
    reconcile
        .set_mesh_enabled_if_current(
            true,
            reconcile.mesh_state_generation.load(Ordering::Acquire),
        )
        .await;
    assert!(reconcile.mesh_gate().load(Ordering::Acquire));
}

#[tokio::test]
async fn snapshot_admission_is_bounded_and_reserves_mesh_until_install_finishes() {
    let reconcile = ReconcileHandle::noop();
    let in_flight_mesh_read = reconcile.mesh_gate_lock().read_owned().await;
    assert!(
        reconcile
            .begin_snapshot_install_until(
                std::time::Instant::now() + std::time::Duration::from_millis(10),
            )
            .await
            .is_none(),
        "snapshot admission must reject before OpenRaft when Mesh readers do not drain"
    );
    assert!(reconcile.mesh_gate().load(Ordering::Acquire));

    drop(in_flight_mesh_read);
    let admission = reconcile
        .begin_snapshot_install_until(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
        .expect("drained Mesh gate should admit a snapshot");
    assert!(
        reconcile
            .mesh_gate_read_until(std::time::Instant::now() + std::time::Duration::from_millis(10))
            .await
            .is_none(),
        "new Mesh work must not enter while snapshot installation owns its reservation"
    );

    drop(admission);
    assert!(
        reconcile
            .mesh_gate_read_until(std::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
            .is_some(),
        "Mesh admission should resume after snapshot installation terminates"
    );
}
