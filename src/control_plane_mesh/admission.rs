use super::*;

impl MeshAwareHttpClient {
    pub(crate) async fn direct_validation_snapshot(
        &self,
        peer: &MeshPeerTarget,
    ) -> (
        DirectValidationState,
        Option<String>,
        tokio::sync::OwnedRwLockReadGuard<Option<String>>,
    ) {
        self.direct_validation_snapshot_until(peer, Instant::now() + Duration::from_secs(60))
            .await
            .expect("status validation snapshot should complete")
    }

    pub(crate) async fn direct_validation_snapshot_until(
        &self,
        peer: &MeshPeerTarget,
        deadline: Instant,
    ) -> Option<(
        DirectValidationState,
        Option<String>,
        tokio::sync::OwnedRwLockReadGuard<Option<String>>,
    )> {
        let membership_guard = crate::control_plane_mesh::await_until(
            deadline,
            self.direct_validation.membership_revision_guard(),
        )
        .await?;
        let membership_revision = membership_guard.clone();
        let state = crate::control_plane_mesh::await_until(
            deadline,
            self.direct_validation.state_at(
                peer,
                self.enforce_direct_validation,
                membership_revision.as_deref(),
            ),
        )
        .await?;
        Some((state, membership_revision, membership_guard))
    }

    pub async fn direct_validation_state_for(
        &self,
        peer: &MeshPeerTarget,
    ) -> DirectValidationState {
        self.direct_validation_snapshot(peer).await.0
    }

    pub async fn mark_direct_validation_success_at(
        &self,
        peer: &MeshPeerTarget,
        membership_revision: Option<String>,
    ) {
        let operation_id = self.circuits.next_operation();
        self.mark_direct_validation_success_with_operation(peer, membership_revision, operation_id)
            .await;
    }

    pub async fn mark_direct_validation_failure_at(
        &self,
        peer: &MeshPeerTarget,
        state: DirectValidationState,
        membership_revision: Option<String>,
    ) {
        let operation_id = self.circuits.next_operation();
        self.mark_direct_validation_failure_with_operation(
            peer,
            state,
            membership_revision,
            operation_id,
        )
        .await;
    }

    pub(super) async fn mark_direct_validation_success_with_operation(
        &self,
        peer: &MeshPeerTarget,
        membership_revision: Option<String>,
        operation_id: u64,
    ) -> bool {
        self.direct_validation
            .record_at_if_newer(
                peer,
                DirectValidationState::Verified,
                membership_revision.as_deref(),
                operation_id,
            )
            .await
    }

    pub(super) fn try_mark_direct_validation_success_with_operation(
        &self,
        peer: &MeshPeerTarget,
        membership_revision: Option<String>,
        operation_id: u64,
    ) -> Option<bool> {
        self.direct_validation.try_record_at_if_newer(
            peer,
            DirectValidationState::Verified,
            membership_revision.as_deref(),
            operation_id,
        )
    }

    pub(super) async fn mark_direct_validation_failure_with_operation(
        &self,
        peer: &MeshPeerTarget,
        state: DirectValidationState,
        membership_revision: Option<String>,
        operation_id: u64,
    ) -> bool {
        self.direct_validation
            .record_at_if_newer(peer, state, membership_revision.as_deref(), operation_id)
            .await
    }

    pub async fn set_membership_revision(&self, revision: Option<String>) {
        self.direct_validation
            .set_membership_revision(revision)
            .await;
    }

    #[cfg(test)]
    pub(super) async fn observe_mesh_gate(&self) -> bool {
        self.observe_mesh_gate_until(Instant::now() + Duration::from_secs(60))
            .await
            .unwrap_or(false)
    }

    pub(super) async fn observe_mesh_gate_until(&self, deadline: Instant) -> Option<bool> {
        let mut reset_guard =
            crate::control_plane_mesh::await_until(deadline, self.mesh_epoch_reset_lock.lock())
                .await?;
        let epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        let previous = *reset_guard;
        let enabled = self.cluster_mesh_enabled.load(Ordering::Acquire);
        if epoch != previous {
            crate::control_plane_mesh::await_until(
                deadline,
                self.circuits.clear_half_open_probes(),
            )
            .await?;
            *reset_guard = epoch;
        }
        Some(enabled)
    }

    async fn before_mesh_attempt_until_with_token(
        &self,
        peer_id: &str,
        enabled: bool,
        health_probe: bool,
        deadline: Instant,
    ) -> Option<(MeshAttemptDecision, u64, Option<u64>)> {
        let _reset_guard =
            crate::control_plane_mesh::await_until(deadline, self.mesh_epoch_reset_lock.lock())
                .await?;
        let epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        let decision = self
            .circuits
            .before_attempt_with_probe_at_epoch_until_with_token(
                peer_id,
                enabled,
                health_probe,
                Some(epoch),
                deadline,
            )
            .await?;
        Some((decision.0, epoch, decision.1))
    }

    #[cfg(test)]
    pub(super) async fn before_mesh_request(
        &self,
        peer_id: &str,
        enabled: bool,
        route: InternalRoute,
    ) -> (MeshAttemptDecision, u64) {
        self.before_mesh_request_until_with_token(
            peer_id,
            enabled,
            route,
            Instant::now() + Duration::from_secs(60),
        )
        .await
        .map(|(decision, epoch, _)| (decision, epoch))
        .expect("test Mesh admission should complete")
    }

    pub(super) async fn before_mesh_request_until_with_token(
        &self,
        peer_id: &str,
        enabled: bool,
        route: InternalRoute,
        deadline: Instant,
    ) -> Option<(MeshAttemptDecision, u64, Option<u64>)> {
        self.before_mesh_attempt_until_with_token(
            peer_id,
            enabled,
            route == InternalRoute::HealthV2,
            deadline,
        )
        .await
    }

    pub(super) fn mesh_probe_guard(
        &self,
        peer_id: &str,
        decision: MeshAttemptDecision,
        epoch: u64,
        probe_id: Option<u64>,
    ) -> Option<MeshHalfOpenProbeGuard> {
        MeshHalfOpenProbeGuard::new(&self.circuits, peer_id, decision, epoch, probe_id)
    }

    pub(super) fn public_probe_guard(
        &self,
        peer_id: &str,
        decision: MeshAttemptDecision,
        probe_id: Option<u64>,
    ) -> Option<PublicHalfOpenProbeGuard> {
        PublicHalfOpenProbeGuard::new(&self.circuits, peer_id, decision, probe_id)
    }

    pub(super) async fn send_peer_direct_preflight_with_admission(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        allow_mesh_when_disabled: bool,
    ) -> Result<reqwest::Response, MeshRequestError> {
        let request_deadline = Instant::now() + request.total_budget;
        let (_, validation_revision, _membership_guard) = self
            .direct_validation_snapshot_until(peer, request_deadline)
            .await
            .ok_or(MeshRequestError::PreDispatchTimeout)?;
        let (decision, epoch, probe_id) = self
            .before_mesh_request_until_with_token(
                &peer.node_id,
                true,
                InternalRoute::HealthV2,
                request_deadline,
            )
            .await
            .ok_or(MeshRequestError::PreDispatchTimeout)?;
        let mut mesh_probe_guard =
            MeshHalfOpenProbeGuard::new(&self.circuits, &peer.node_id, decision, epoch, probe_id);
        if matches!(
            decision,
            MeshAttemptDecision::SkipOpen | MeshAttemptDecision::Quarantined
        ) {
            return Err(MeshRequestError::CircuitOpen {
                path: "Direct Mesh",
                dispatched: false,
            });
        }
        let result = self
            .send_peer_direct_request_with_options(
                peer,
                PeerDirectPath::RealityMesh,
                request,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                None,
                allow_mesh_when_disabled,
                request_deadline,
            )
            .await;
        if matches!(decision, MeshAttemptDecision::Probe) {
            self.release_mesh_probe_guard_until(
                &mut mesh_probe_guard,
                &peer.node_id,
                epoch,
                request_deadline,
            )
            .await;
        }
        if !self
            .mesh_epoch_is_current_until(epoch, request_deadline)
            .await
        {
            return result;
        }
        let operation_id = self.circuits.next_operation();
        if let Err(error) = &result {
            let validation_state = match error {
                MeshRequestError::PreDispatchAuth(_)
                | MeshRequestError::PreDispatchTimeout
                | MeshRequestError::InvalidTarget(_)
                | MeshRequestError::CircuitOpen { .. } => return result,
                MeshRequestError::Auth(_) | MeshRequestError::Protocol(_) => {
                    DirectValidationState::ProtocolRejected
                }
                _ => DirectValidationState::TransportFailed,
            };
            let recorded = match validation_state {
                DirectValidationState::ProtocolRejected => crate::control_plane_mesh::await_until(
                    request_deadline,
                    self.circuits
                        .record_protocol_failure_at(&peer.node_id, operation_id),
                )
                .await
                .flatten()
                .is_some(),
                DirectValidationState::TransportFailed => crate::control_plane_mesh::await_until(
                    request_deadline,
                    self.circuits
                        .record_retryable_failure_at(&peer.node_id, operation_id),
                )
                .await
                .flatten()
                .is_some(),
                _ => unreachable!("preflight failure state is classified above"),
            };
            if !recorded {
                match validation_state {
                    DirectValidationState::ProtocolRejected => self.spawn_protocol_failure_cleanup(
                        peer,
                        epoch,
                        validation_revision.clone(),
                        operation_id,
                    ),
                    DirectValidationState::TransportFailed => self.spawn_retryable_failure_cleanup(
                        peer,
                        epoch,
                        validation_revision.clone(),
                        operation_id,
                    ),
                    _ => unreachable!("preflight failure state is classified above"),
                }
            }
            let cleanup_revision = validation_revision.clone();
            let validation_recorded = crate::control_plane_mesh::await_until(
                request_deadline,
                self.mark_direct_validation_failure_with_operation(
                    peer,
                    validation_state,
                    validation_revision,
                    operation_id,
                ),
            )
            .await
            .is_some_and(|recorded| recorded);
            if !validation_recorded {
                self.spawn_validation_failure_cleanup(
                    peer,
                    epoch,
                    validation_state,
                    cleanup_revision,
                    operation_id,
                );
            }
        } else {
            let cleanup_revision = validation_revision.clone();
            let breaker_recorded = crate::control_plane_mesh::await_until(
                request_deadline,
                self.circuits.record_success_at(&peer.node_id, operation_id),
            )
            .await
            .flatten()
            .is_some();
            let validation_recorded = crate::control_plane_mesh::await_until(
                request_deadline,
                self.mark_direct_validation_success_with_operation(
                    peer,
                    validation_revision,
                    operation_id,
                ),
            )
            .await
            .is_some_and(|recorded| recorded);
            if !breaker_recorded || !validation_recorded {
                self.spawn_validation_success_cleanup(peer, epoch, cleanup_revision, operation_id);
            }
        }
        result
    }

    #[cfg(test)]
    pub(super) async fn mesh_attempt_is_current(&self, epoch: u64) -> bool {
        self.mesh_attempt_is_current_until(epoch, Instant::now() + Duration::from_secs(60))
            .await
    }

    pub(super) async fn mesh_attempt_is_current_until(
        &self,
        epoch: u64,
        deadline: Instant,
    ) -> bool {
        let Some(_reset_guard) =
            crate::control_plane_mesh::await_until(deadline, self.mesh_epoch_reset_lock.lock())
                .await
        else {
            return false;
        };
        self.mesh_gate_matches(epoch)
    }

    pub(super) async fn mesh_epoch_is_current_until(&self, epoch: u64, deadline: Instant) -> bool {
        let Some(_reset_guard) =
            crate::control_plane_mesh::await_until(deadline, self.mesh_epoch_reset_lock.lock())
                .await
        else {
            return false;
        };
        self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch
    }

    pub(super) async fn release_half_open_probe_for_epoch_until(
        &self,
        peer_id: &str,
        epoch: u64,
        probe_id: Option<u64>,
        deadline: Instant,
    ) -> bool {
        let Some(probe_id) = probe_id else {
            return false;
        };
        let Some(_reset_guard) =
            crate::control_plane_mesh::await_until(deadline, self.mesh_epoch_reset_lock.lock())
                .await
        else {
            return false;
        };
        crate::control_plane_mesh::await_until(
            deadline,
            self.circuits
                .release_half_open_probe_for_epoch(peer_id, epoch, probe_id),
        )
        .await
        .unwrap_or(false)
    }

    pub(super) async fn release_mesh_probe_guard_until(
        &self,
        guard: &mut Option<MeshHalfOpenProbeGuard>,
        peer_id: &str,
        epoch: u64,
        deadline: Instant,
    ) {
        let probe_id = guard.as_ref().map(MeshHalfOpenProbeGuard::probe_id);
        if self
            .release_half_open_probe_for_epoch_until(peer_id, epoch, probe_id, deadline)
            .await
            && let Some(guard) = guard.as_mut()
        {
            guard.disarm();
        }
    }

    pub(super) async fn release_public_probe_guard_until(
        &self,
        guard: &mut Option<PublicHalfOpenProbeGuard>,
        peer_id: &str,
        deadline: Instant,
    ) {
        let probe_id = guard.as_ref().map(PublicHalfOpenProbeGuard::probe_id);
        let Some(probe_id) = probe_id else {
            return;
        };
        if crate::control_plane_mesh::await_until(
            deadline,
            self.circuits
                .release_public_half_open_probe(peer_id, probe_id),
        )
        .await
        .unwrap_or(false)
            && let Some(guard) = guard.as_mut()
        {
            guard.disarm();
        }
    }

    #[cfg(test)]
    pub(super) async fn release_half_open_probe_for_epoch(&self, peer_id: &str, epoch: u64) {
        let probe_id = self
            .circuits
            .peers
            .lock()
            .await
            .get(peer_id)
            .and_then(|circuit| circuit.half_open_probe_id);
        self.release_half_open_probe_for_epoch_until(
            peer_id,
            epoch,
            probe_id,
            Instant::now() + Duration::from_secs(60),
        )
        .await;
    }

    pub(super) async fn mesh_read_guard_for_epoch_until(
        &self,
        epoch: u64,
        deadline: Instant,
    ) -> Option<tokio::sync::OwnedRwLockReadGuard<()>> {
        let guard = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.mesh_gate_lock.clone().read_owned(),
        )
        .await
        .ok()?;
        (self.cluster_mesh_enabled.load(Ordering::Acquire)
            && self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch)
            .then_some(guard)
    }

    pub(super) async fn mesh_direct_read_guard_until(
        &self,
        deadline: Instant,
    ) -> Result<tokio::sync::OwnedRwLockReadGuard<()>, MeshRequestError> {
        let epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        if !self.cluster_mesh_enabled.load(Ordering::Acquire) {
            return Err(MeshRequestError::InvalidTarget(
                "Mesh is disabled by the cluster gate".into(),
            ));
        }
        let Some(guard) = self.mesh_read_guard_for_epoch_until(epoch, deadline).await else {
            return Err(if self.cluster_mesh_enabled.load(Ordering::Acquire) {
                MeshRequestError::PreDispatchTimeout
            } else {
                MeshRequestError::InvalidTarget("Mesh is disabled by the cluster gate".into())
            });
        };
        Ok(guard)
    }

    pub(super) async fn mesh_read_guard_for_path_until(
        &self,
        path: PeerDirectPath,
        deadline: Instant,
    ) -> Result<Option<tokio::sync::OwnedRwLockReadGuard<()>>, MeshRequestError> {
        if path == PeerDirectPath::RealityMesh {
            self.mesh_direct_read_guard_until(deadline).await.map(Some)
        } else {
            Ok(None)
        }
    }
}
