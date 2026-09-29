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
        let Some(state) = super::await_until(
            deadline,
            self.circuits.record_retryable_failure(&peer.node_id),
        )
        .await
        else {
            drop(gate_guard);
            return;
        };
        if !self.mesh_gate_matches(epoch) {
            drop(gate_guard);
            return;
        }
        let _ = super::await_until(
            deadline,
            self.mark_direct_validation_failure_at(
                peer,
                DirectValidationState::TransportFailed,
                validation_revision,
            ),
        )
        .await;
        drop(gate_guard);
        if !self.mesh_gate_matches(epoch) {
            return;
        }
        self.record_sample_until(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                false,
                Duration::ZERO,
                false,
                false,
                None,
            ),
            deadline,
        )
        .await;
        if !self.mesh_gate_matches(epoch) {
            return;
        }
        self.record_mesh_reason_until(peer, mesh_reason, deadline)
            .await;
        if let Some(telemetry) = &self.telemetry {
            if !self.mesh_gate_matches(epoch) {
                return;
            }
            let message = if state == BreakerState::Open {
                format!("Mesh breaker opened after retryable transport failure: {reason}")
            } else {
                format!("Mesh transport failure: {reason}")
            };
            let _ = super::await_until(
                deadline,
                telemetry.set_breaker(
                    &peer.node_id,
                    state,
                    (state == BreakerState::Open).then_some(message),
                ),
            )
            .await;
        }
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
        self.record_sample_until(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                false,
                Duration::ZERO,
                false,
                false,
                None,
            ),
            deadline,
        )
        .await;
        if self.mesh_gate_matches(epoch) {
            self.record_mesh_reason_until(peer, MeshPeerReason::ProtocolRejected, deadline)
                .await;
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

    pub(super) async fn record_sample_until(
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
        if self.mesh_gate_matches(epoch) {
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
        if self.cluster_mesh_epoch.load(Ordering::Acquire) != epoch {
            return;
        }
        self.record_sample_until(peer, sample, deadline).await;
        if self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch
            && fallback
            && peer.mesh_base_url.is_some()
        {
            self.record_mesh_reason_until(peer, MeshPeerReason::FallbackActive, deadline)
                .await;
        }
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
