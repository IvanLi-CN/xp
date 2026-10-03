use super::*;

impl ReconcileHandle {
    pub fn mesh_gate(&self) -> Arc<AtomicBool> {
        self.mesh_enabled.clone()
    }

    pub fn mesh_gate_epoch(&self) -> Arc<AtomicU64> {
        self.mesh_enabled_epoch.clone()
    }

    pub fn mesh_gate_lock(&self) -> Arc<RwLock<()>> {
        self.mesh_gate_lock.clone()
    }

    pub fn mesh_epoch_barrier(&self) -> Arc<RwLock<()>> {
        self.mesh_epoch_barrier.clone()
    }

    pub async fn mesh_gate_read(&self) -> Option<tokio::sync::OwnedRwLockReadGuard<()>> {
        self.mesh_gate_read_until(std::time::Instant::now() + Duration::from_secs(5))
            .await
    }

    pub async fn mesh_gate_read_until(
        &self,
        deadline: std::time::Instant,
    ) -> Option<tokio::sync::OwnedRwLockReadGuard<()>> {
        if !self.mesh_enabled.load(Ordering::Acquire) {
            return None;
        }
        let guard = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.mesh_gate_lock.clone().read_owned(),
        )
        .await
        .ok()?;
        self.mesh_enabled.load(Ordering::Acquire).then_some(guard)
    }

    pub async fn initialize_mesh_gate(&self, enabled: bool) {
        let _ = self
            .initialize_mesh_gate_until(
                enabled,
                std::time::Instant::now() + Duration::from_secs(60),
            )
            .await;
    }

    pub async fn initialize_mesh_gate_if_unset(&self, enabled: bool) {
        if self.mesh_gate_authoritative.load(Ordering::Acquire) {
            return;
        }
        let _ = self
            .initialize_mesh_gate_if_unset_until(
                enabled,
                std::time::Instant::now() + Duration::from_secs(60),
            )
            .await;
    }

    pub async fn initialize_mesh_gate_until(
        &self,
        enabled: bool,
        deadline: std::time::Instant,
    ) -> bool {
        if self.mesh_gate_authoritative.load(Ordering::Acquire)
            && self.mesh_enabled.load(Ordering::Acquire) == enabled
        {
            return true;
        }
        let Some(_gate_lock) = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.mesh_gate_lock.clone().write_owned(),
        )
        .await
        .ok() else {
            self.fail_closed_if_disabled(enabled);
            return false;
        };
        let Some(_epoch_barrier) = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.mesh_epoch_barrier.clone().write_owned(),
        )
        .await
        .ok() else {
            self.fail_closed_if_disabled(enabled);
            return false;
        };
        self.mesh_state_applied.store(true, Ordering::Release);
        self.mesh_gate_authoritative.store(true, Ordering::Release);
        self.apply_mesh_enabled_locked(enabled);
        true
    }

    pub async fn initialize_mesh_gate_if_unset_until(
        &self,
        enabled: bool,
        deadline: std::time::Instant,
    ) -> bool {
        if self.mesh_gate_authoritative.load(Ordering::Acquire) {
            return true;
        }
        let Some(_gate_lock) = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.mesh_gate_lock.clone().write_owned(),
        )
        .await
        .ok() else {
            self.fail_closed_if_disabled(enabled);
            return false;
        };
        if !self.mesh_gate_authoritative.load(Ordering::Acquire) {
            let Some(_epoch_barrier) = tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                self.mesh_epoch_barrier.clone().write_owned(),
            )
            .await
            .ok() else {
                self.fail_closed_if_disabled(enabled);
                return false;
            };
            self.mesh_state_applied.store(true, Ordering::Release);
            self.mesh_gate_authoritative.store(true, Ordering::Release);
            self.apply_mesh_enabled_locked(enabled);
        }
        true
    }

    pub async fn hold_mesh_gate_until_raft_state(&self) {
        let _ = self
            .hold_mesh_gate_until_raft_state_until(
                std::time::Instant::now() + Duration::from_secs(60),
            )
            .await;
    }

    pub async fn hold_mesh_gate_until_raft_state_until(
        &self,
        deadline: std::time::Instant,
    ) -> bool {
        let Some(_gate_lock) = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.mesh_gate_lock.clone().write_owned(),
        )
        .await
        .ok() else {
            return false;
        };
        let Some(_epoch_barrier) = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.mesh_epoch_barrier.clone().write_owned(),
        )
        .await
        .ok() else {
            return false;
        };
        self.mesh_state_applied.store(false, Ordering::Release);
        self.mesh_gate_authoritative.store(false, Ordering::Release);
        self.mesh_enabled.store(false, Ordering::Release);
        self.refresh_reverse_gate();
        true
    }

    pub(crate) fn note_mesh_state_applied(&self) {
        self.mesh_state_generation.fetch_add(1, Ordering::AcqRel);
        self.mesh_state_applied.store(true, Ordering::Release);
    }

    pub(super) async fn set_mesh_enabled_if_current(&self, enabled: bool, generation: u64) {
        if !self.mesh_state_applied.load(Ordering::Acquire) {
            return;
        }
        if self.mesh_gate_authoritative.load(Ordering::Acquire)
            && self.mesh_enabled.load(Ordering::Acquire) == enabled
        {
            return;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let Some(_gate_lock) = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.mesh_gate_lock.clone().write_owned(),
        )
        .await
        .ok() else {
            self.fail_closed_if_disabled(enabled);
            return;
        };
        if !self.mesh_state_applied.load(Ordering::Acquire)
            || self.mesh_state_generation.load(Ordering::Acquire) != generation
        {
            return;
        }
        let Some(_epoch_barrier) = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.mesh_epoch_barrier.clone().write_owned(),
        )
        .await
        .ok() else {
            self.fail_closed_if_disabled(enabled);
            return;
        };
        if self.mesh_state_applied.load(Ordering::Acquire)
            && self.mesh_state_generation.load(Ordering::Acquire) == generation
        {
            self.mesh_gate_authoritative.store(true, Ordering::Release);
            self.apply_mesh_enabled_locked(enabled);
        }
    }

    fn apply_mesh_enabled_locked(&self, enabled: bool) {
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

    fn fail_closed_if_disabled(&self, enabled: bool) {
        if !enabled {
            self.mesh_enabled.store(false, Ordering::Release);
            self.refresh_reverse_gate();
        }
    }
}
