use super::*;

impl MeshAwareHttpClient {
    pub fn with_mesh_gate_epoch(
        mut self,
        gate: Arc<AtomicBool>,
        epoch: Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        self.cluster_mesh_enabled = gate;
        self.cluster_mesh_epoch = epoch;
        self
    }

    pub(super) async fn observe_mesh_gate(&self) -> bool {
        let mut reset_guard = self.mesh_epoch_reset_lock.lock().await;
        let epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        let previous = *reset_guard;
        let enabled = self.cluster_mesh_enabled.load(Ordering::Acquire);
        if enabled && epoch != previous {
            self.circuits.reset_all().await;
        }
        if epoch != previous {
            *reset_guard = epoch;
        }
        enabled
    }

    pub(super) async fn before_mesh_attempt(
        &self,
        peer_id: &str,
        enabled: bool,
    ) -> (MeshAttemptDecision, u64) {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        let epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        let decision = self.circuits.before_attempt(peer_id, enabled).await;
        (decision, epoch)
    }

    pub(super) async fn release_half_open_probe_for_epoch(&self, peer_id: &str, epoch: u64) {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        if self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch {
            self.circuits.release_half_open_probe(peer_id).await;
        }
    }
}
