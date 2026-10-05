use super::*;

impl ReconcileHandle {
    pub fn noop() -> Self {
        Self {
            tx: None,
            restart_requested: Arc::new(AtomicBool::new(false)),
            reverse_enabled: Arc::new(AtomicBool::new(true)),
            reverse_supervisor_enabled: Arc::new(AtomicBool::new(true)),
            reverse_runtime_ready: Arc::new(AtomicBool::new(true)),
            reverse_recovery_required: Arc::new(AtomicBool::new(false)),
            reverse_operator_enabled: Arc::new(AtomicBool::new(true)),
            reverse_links: ReverseLinkRuntime::default(),
            mesh_enabled: Arc::new(AtomicBool::new(true)),
            mesh_enabled_epoch: Arc::new(AtomicU64::new(0)),
            mesh_state_generation: Arc::new(AtomicU64::new(0)),
            mesh_generation_lock: Arc::new(std::sync::Mutex::new(())),
            mesh_state_applied: Arc::new(AtomicBool::new(true)),
            mesh_gate_authoritative: Arc::new(AtomicBool::new(true)),
            snapshot_installing: Arc::new(AtomicBool::new(false)),
            mesh_gate_lock: Arc::new(RwLock::new(())),
            mesh_epoch_barrier: Arc::new(RwLock::new(())),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_sender(tx: mpsc::UnboundedSender<ReconcileRequest>) -> Self {
        Self {
            tx: Some(tx),
            restart_requested: Arc::new(AtomicBool::new(false)),
            reverse_enabled: Arc::new(AtomicBool::new(true)),
            reverse_supervisor_enabled: Arc::new(AtomicBool::new(true)),
            reverse_runtime_ready: Arc::new(AtomicBool::new(true)),
            reverse_recovery_required: Arc::new(AtomicBool::new(false)),
            reverse_operator_enabled: Arc::new(AtomicBool::new(true)),
            reverse_links: ReverseLinkRuntime::default(),
            mesh_enabled: Arc::new(AtomicBool::new(true)),
            mesh_enabled_epoch: Arc::new(AtomicU64::new(0)),
            mesh_state_generation: Arc::new(AtomicU64::new(0)),
            mesh_generation_lock: Arc::new(std::sync::Mutex::new(())),
            mesh_state_applied: Arc::new(AtomicBool::new(true)),
            mesh_gate_authoritative: Arc::new(AtomicBool::new(true)),
            snapshot_installing: Arc::new(AtomicBool::new(false)),
            mesh_gate_lock: Arc::new(RwLock::new(())),
            mesh_epoch_barrier: Arc::new(RwLock::new(())),
        }
    }
}
