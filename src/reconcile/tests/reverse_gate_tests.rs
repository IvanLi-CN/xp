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
