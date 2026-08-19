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
