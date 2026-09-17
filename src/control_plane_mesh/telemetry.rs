use super::*;

impl MeshAwareHttpClient {
    pub(super) async fn record_mesh_transport_failure(
        &self,
        peer: &MeshPeerTarget,
        mesh_reason: MeshPeerReason,
        reason: String,
        epoch: u64,
    ) {
        let _gate_lock = self.mesh_gate_lock.lock().await;
        let state = {
            if !self.mesh_gate_matches(epoch) {
                return;
            }
            self.circuits.record_retryable_failure(&peer.node_id).await
        };
        self.record_sample(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                false,
                Duration::ZERO,
                false,
                false,
                None,
            ),
        )
        .await;
        self.record_mesh_reason(peer, mesh_reason).await;
        if let Some(telemetry) = &self.telemetry {
            let message = if state == BreakerState::Open {
                format!("Mesh breaker opened after retryable transport failure: {reason}")
            } else {
                format!("Mesh transport failure: {reason}")
            };
            let _ = telemetry
                .set_breaker(
                    &peer.node_id,
                    state,
                    (state == BreakerState::Open).then_some(message),
                )
                .await;
        }
    }

    pub(super) async fn record_mesh_protocol_failure(&self, peer: &MeshPeerTarget, epoch: u64) {
        let _gate_lock = self.mesh_gate_lock.lock().await;
        if !self.mesh_gate_matches(epoch) {
            return;
        }
        self.record_sample(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                false,
                Duration::ZERO,
                false,
                false,
                None,
            ),
        )
        .await;
        self.record_mesh_reason(peer, MeshPeerReason::ProtocolRejected)
            .await;
    }

    pub(super) async fn record_mesh_reason(&self, peer: &MeshPeerTarget, reason: MeshPeerReason) {
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .set_mesh_reason(&peer.node_id, peer.mesh_base_url.as_deref(), reason)
                .await;
        }
    }

    pub(super) async fn record_sample(&self, peer: &MeshPeerTarget, sample: MeshTelemetrySample) {
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .record_sample(&peer.node_id, &peer.node_name, sample)
                .await;
        }
        if sample.success && sample.path == TelemetryPath::Mesh {
            self.record_mesh_reason(peer, MeshPeerReason::MeshAvailable)
                .await;
        }
    }

    pub(super) async fn record_terminal_failure_for_epoch(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
    ) {
        let _gate_lock = self.mesh_gate_lock.lock().await;
        if !self.mesh_gate_matches(epoch) {
            return;
        }
        self.record_terminal_failure(peer).await;
    }

    pub(super) async fn record_terminal_failure(&self, peer: &MeshPeerTarget) {
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .record_terminal_failure(&peer.node_id, &peer.node_name)
                .await;
        }
    }
}
