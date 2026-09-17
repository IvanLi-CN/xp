use super::*;

impl ReconcileHandle {
    pub fn mesh_gate(&self) -> Arc<AtomicBool> {
        self.mesh_enabled.clone()
    }

    pub fn mesh_gate_epoch(&self) -> Arc<AtomicU64> {
        self.mesh_enabled_epoch.clone()
    }

    pub fn initialize_mesh_gate(&self, enabled: bool) {
        self.mesh_gate_authoritative.store(true, Ordering::Release);
        self.set_mesh_enabled(enabled);
    }

    pub fn initialize_mesh_gate_if_unset(&self, enabled: bool) {
        if !self.mesh_gate_authoritative.swap(true, Ordering::AcqRel) {
            self.set_mesh_enabled(enabled);
        }
    }

    pub fn hold_mesh_gate_until_raft_state(&self) {
        self.mesh_gate_authoritative.store(false, Ordering::Release);
        self.mesh_enabled.store(false, Ordering::Release);
        self.refresh_reverse_gate();
    }

    pub(super) fn set_mesh_enabled(&self, enabled: bool) {
        if !self.mesh_gate_authoritative.load(Ordering::Acquire) {
            self.mesh_enabled.store(false, Ordering::Release);
            self.refresh_reverse_gate();
            return;
        }
        let was_enabled = self.mesh_enabled.swap(enabled, Ordering::AcqRel);
        if was_enabled != enabled {
            self.mesh_enabled_epoch.fetch_add(1, Ordering::AcqRel);
        }
        if enabled && !was_enabled {
            self.reverse_runtime_ready.store(false, Ordering::Release);
        }
        self.refresh_reverse_gate();
    }
}
