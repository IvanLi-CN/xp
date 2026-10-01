use super::*;

impl MeshAwareHttpClient {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn record_mesh_transport_failure(
        &self,
        peer: &MeshPeerTarget,
        mesh_reason: MeshPeerReason,
        reason: String,
        epoch: u64,
        gate_guard: tokio::sync::OwnedRwLockReadGuard<()>,
        validation_revision: Option<String>,
        deadline: Instant,
    ) {
        if !self.mesh_gate_matches(epoch) {
            drop(gate_guard);
            return;
        }
        let operation_id = self.circuits.next_operation();
        let breaker_result = super::await_until(
            deadline,
            self.circuits
                .record_retryable_failure_at(&peer.node_id, operation_id),
        )
        .await;
        let Some(state) = breaker_result.flatten() else {
            if breaker_result.is_none() {
                self.spawn_retryable_failure_cleanup(
                    peer,
                    epoch,
                    validation_revision,
                    operation_id,
                );
            }
            drop(gate_guard);
            return;
        };
        if !self.mesh_gate_matches(epoch) {
            drop(gate_guard);
            return;
        }
        let cleanup_revision = validation_revision.clone();
        let recorded = super::await_until(
            deadline,
            self.mark_direct_validation_failure_with_operation(
                peer,
                DirectValidationState::TransportFailed,
                validation_revision,
                operation_id,
            ),
        )
        .await
            == Some(true);
        if !recorded {
            self.spawn_validation_failure_cleanup(
                peer,
                epoch,
                DirectValidationState::TransportFailed,
                cleanup_revision,
                operation_id,
            );
        }
        drop(gate_guard);
        self.record_sample_for_epoch_until(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                false,
                Duration::ZERO,
                false,
                false,
                None,
            ),
            epoch,
            deadline,
        )
        .await;
        self.record_mesh_reason_for_epoch_until(peer, mesh_reason, epoch, deadline)
            .await;
        let message = if state == BreakerState::Open {
            format!("Mesh breaker opened after retryable transport failure: {reason}")
        } else {
            format!("Mesh transport failure: {reason}")
        };
        self.set_mesh_breaker_for_epoch_until(
            peer,
            state,
            (state == BreakerState::Open).then_some(message),
            epoch,
            deadline,
        )
        .await;
    }

    pub(super) async fn record_mesh_protocol_failure(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        deadline: Instant,
    ) {
        if !self.mesh_gate_matches(epoch) {
            return;
        }
        self.record_sample_for_epoch_until(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                false,
                Duration::ZERO,
                false,
                false,
                None,
            ),
            epoch,
            deadline,
        )
        .await;
        self.record_mesh_reason_for_epoch_until(
            peer,
            MeshPeerReason::ProtocolRejected,
            epoch,
            deadline,
        )
        .await;
    }

    async fn record_mesh_reason_for_epoch_until(
        &self,
        peer: &MeshPeerTarget,
        reason: MeshPeerReason,
        epoch: u64,
        deadline: Instant,
    ) {
        if let Some(_epoch_guard) = self.try_mesh_epoch_guard(epoch, true) {
            self.record_mesh_reason_until(peer, reason, deadline).await;
        }
    }

    async fn record_mesh_reason_until(
        &self,
        peer: &MeshPeerTarget,
        reason: MeshPeerReason,
        deadline: Instant,
    ) {
        if let Some(telemetry) = &self.telemetry {
            let _ = super::await_until(
                deadline,
                telemetry.set_mesh_reason(&peer.node_id, peer.mesh_base_url.as_deref(), reason),
            )
            .await;
        }
    }

    async fn record_sample_until(
        &self,
        peer: &MeshPeerTarget,
        sample: MeshTelemetrySample,
        deadline: Instant,
    ) {
        if let Some(telemetry) = &self.telemetry {
            let _ = super::await_until(
                deadline,
                telemetry.record_sample(&peer.node_id, &peer.node_name, sample),
            )
            .await;
        }
    }

    pub(super) async fn record_sample_for_epoch_until(
        &self,
        peer: &MeshPeerTarget,
        sample: MeshTelemetrySample,
        epoch: u64,
        deadline: Instant,
    ) {
        let Some(_epoch_guard) = self.try_mesh_epoch_guard(epoch, true) else {
            return;
        };
        self.record_sample_until(peer, sample, deadline).await;
        if sample.success && sample.path == TelemetryPath::Mesh {
            self.record_mesh_reason_until(peer, MeshPeerReason::MeshAvailable, deadline)
                .await;
        }
    }

    pub(super) async fn record_terminal_failure_for_epoch(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        deadline: Instant,
    ) {
        if let Some(_epoch_guard) = self.try_mesh_epoch_guard(epoch, true) {
            self.record_terminal_failure_until(peer, deadline).await;
        }
    }

    pub(super) async fn record_public_sample_for_epoch(
        &self,
        peer: &MeshPeerTarget,
        sample: MeshTelemetrySample,
        epoch: u64,
        fallback: bool,
        deadline: Instant,
    ) {
        let Some(_epoch_guard) = self.try_mesh_epoch_guard(epoch, false) else {
            return;
        };
        self.record_sample_until(peer, sample, deadline).await;
        if fallback
            && peer.mesh_base_url.is_some()
            && self.cluster_mesh_enabled.load(Ordering::Acquire)
        {
            self.record_mesh_reason_until(peer, MeshPeerReason::FallbackActive, deadline)
                .await;
        }
    }

    pub(super) async fn set_mesh_breaker_for_epoch_until(
        &self,
        peer: &MeshPeerTarget,
        state: BreakerState,
        event_message: Option<String>,
        epoch: u64,
        deadline: Instant,
    ) {
        let Some(_epoch_guard) = self.try_mesh_epoch_guard(epoch, true) else {
            return;
        };
        let Some(telemetry) = &self.telemetry else {
            return;
        };
        let _ = super::await_until(
            deadline,
            telemetry.set_breaker(&peer.node_id, state, event_message),
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn record_public_outcome_for_epoch(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        success: bool,
        fallback: bool,
        updates_active_path: bool,
        epoch: u64,
        deadline: Instant,
    ) {
        self.record_public_sample_for_epoch(
            peer,
            telemetry_sample(
                TelemetryPath::Public,
                success,
                started.elapsed(),
                fallback,
                updates_active_path,
                None,
            ),
            epoch,
            fallback,
            deadline,
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn public_success_telemetry_callback(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        fallback: bool,
        updates_active_path: bool,
        public_epoch: u64,
        operation_id: u64,
        public_probe_guard: Option<PublicHalfOpenProbeGuard>,
        deadline: Instant,
    ) -> crate::mesh_gate_body::FinishCallback {
        let client = self.clone();
        let peer = peer.clone();
        Box::new(move |outcome| {
            if outcome != crate::mesh_gate_body::BodyFinish::Complete {
                return;
            }
            let completion_client = client.clone();
            client.dispatch_completion(async move {
                completion_client
                    .record_public_success_after_body(
                        &peer,
                        started,
                        fallback,
                        updates_active_path,
                        public_epoch,
                        operation_id,
                        public_probe_guard,
                        deadline,
                    )
                    .await;
            });
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_public_success_after_body(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        fallback: bool,
        updates_active_path: bool,
        public_epoch: u64,
        operation_id: u64,
        mut public_probe_guard: Option<PublicHalfOpenProbeGuard>,
        deadline: Instant,
    ) {
        let Some(_epoch_guard) = self
            .mesh_epoch_guard_until(public_epoch, deadline, false)
            .await
        else {
            return;
        };
        let public_breaker_result = super::await_until(
            deadline,
            self.circuits
                .record_public_success_at(&peer.node_id, operation_id),
        )
        .await;
        if public_breaker_result.is_none() {
            self.circuits
                .spawn_public_success_cleanup(&peer.node_id, operation_id);
        }
        if public_breaker_result.is_some()
            && let Some(guard) = public_probe_guard.as_mut()
        {
            guard.disarm();
        }
        if let Some(public_breaker) = public_breaker_result.flatten()
            && let Some(telemetry) = &self.telemetry
        {
            let _ = super::await_until(
                deadline,
                telemetry.set_public_breaker(&peer.node_id, public_breaker, None),
            )
            .await;
        }
        self.record_public_outcome_for_epoch(
            peer,
            started,
            true,
            fallback,
            updates_active_path,
            public_epoch,
            deadline,
        )
        .await;
    }

    pub(super) async fn record_terminal_failure_until(
        &self,
        peer: &MeshPeerTarget,
        deadline: Instant,
    ) {
        if let Some(telemetry) = &self.telemetry {
            let _ = super::await_until(
                deadline,
                telemetry.record_terminal_failure(&peer.node_id, &peer.node_name),
            )
            .await;
        }
    }
}
