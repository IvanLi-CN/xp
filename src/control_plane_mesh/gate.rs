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

    pub fn with_mesh_gate_lock(mut self, lock: Arc<tokio::sync::RwLock<()>>) -> Self {
        self.mesh_gate_lock = lock;
        self
    }

    pub(super) async fn observe_mesh_gate(&self) -> bool {
        let mut reset_guard = self.mesh_epoch_reset_lock.lock().await;
        let epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        let previous = *reset_guard;
        let enabled = self.cluster_mesh_enabled.load(Ordering::Acquire);
        if epoch != previous {
            self.circuits.clear_half_open_probes().await;
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

    pub(super) async fn mesh_attempt_is_current(&self, epoch: u64) -> bool {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        self.mesh_gate_matches(epoch)
    }

    pub(super) fn mesh_gate_matches(&self, epoch: u64) -> bool {
        self.cluster_mesh_enabled.load(Ordering::Acquire)
            && self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch
    }

    pub(super) async fn with_mesh_send<T, F, Fut>(&self, epoch: u64, send: F) -> Option<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let _gate_lock = self.mesh_gate_lock.read().await;
        if !self.cluster_mesh_enabled.load(Ordering::Acquire)
            || self.cluster_mesh_epoch.load(Ordering::Acquire) != epoch
        {
            return None;
        }
        Some(send().await)
    }

    pub(super) async fn release_half_open_probe_for_epoch(&self, peer_id: &str, epoch: u64) {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        if self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch {
            self.circuits.release_half_open_probe(peer_id).await;
        }
    }
}

impl PeerCircuitBreakers {
    async fn clear_half_open_probes(&self) {
        let mut peers = self.peers.lock().await;
        for circuit in peers.values_mut() {
            circuit.half_open_in_flight = false;
        }
    }
}
