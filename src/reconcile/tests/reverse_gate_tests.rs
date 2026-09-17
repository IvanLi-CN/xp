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

#[test]
fn mesh_gate_can_be_initialized_from_persisted_cluster_state() {
    let reconcile = ReconcileHandle::noop();
    assert!(reconcile.mesh_gate().load(Ordering::Acquire));

    reconcile.initialize_mesh_gate(false);
    assert!(!reconcile.mesh_gate().load(Ordering::Acquire));

    reconcile.initialize_mesh_gate(true);
    assert!(reconcile.mesh_gate().load(Ordering::Acquire));
}

#[test]
fn mesh_gate_only_resets_reverse_readiness_on_enable_transition() {
    let reconcile = ReconcileHandle::noop();
    reconcile.set_reverse_runtime_ready(false);

    reconcile.initialize_mesh_gate(true);
    assert!(!reconcile.reverse_gate().load(Ordering::Acquire));

    reconcile.set_reverse_runtime_ready(true);
    assert!(reconcile.reverse_gate().load(Ordering::Acquire));

    reconcile.initialize_mesh_gate(false);
    reconcile.initialize_mesh_gate(true);
    assert!(!reconcile.reverse_gate().load(Ordering::Acquire));
}
