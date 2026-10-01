use super::*;

const POST_DEADLINE_CLEANUP_WAIT: Duration = Duration::from_millis(100);

impl MeshAwareHttpClient {
    pub(super) fn spawn_protocol_failure_cleanup(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let deadline = Instant::now() + POST_DEADLINE_CLEANUP_WAIT;
                let Some(_epoch_guard) = client.mesh_epoch_guard_until(epoch, deadline, true).await
                else {
                    return;
                };
                let _ = crate::control_plane_mesh::await_until(
                    deadline,
                    client
                        .circuits
                        .record_protocol_failure_at(&peer.node_id, operation_id),
                )
                .await;
                let _ = crate::control_plane_mesh::await_until(
                    deadline,
                    client.mark_direct_validation_failure_with_operation(
                        &peer,
                        DirectValidationState::ProtocolRejected,
                        validation_revision,
                        operation_id,
                    ),
                )
                .await;
            });
        }
    }

    pub(super) fn spawn_retryable_failure_cleanup(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let deadline = Instant::now() + POST_DEADLINE_CLEANUP_WAIT;
                let Some(_epoch_guard) = client.mesh_epoch_guard_until(epoch, deadline, true).await
                else {
                    return;
                };
                let _ = crate::control_plane_mesh::await_until(
                    deadline,
                    client
                        .circuits
                        .record_retryable_failure_at(&peer.node_id, operation_id),
                )
                .await;
                let _ = crate::control_plane_mesh::await_until(
                    deadline,
                    client.mark_direct_validation_failure_with_operation(
                        &peer,
                        DirectValidationState::TransportFailed,
                        validation_revision,
                        operation_id,
                    ),
                )
                .await;
            });
        }
    }

    pub(super) fn spawn_validation_failure_cleanup(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        state: DirectValidationState,
        validation_revision: Option<String>,
        operation_id: u64,
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let deadline = Instant::now() + POST_DEADLINE_CLEANUP_WAIT;
                let Some(_epoch_guard) = client.mesh_epoch_guard_until(epoch, deadline, true).await
                else {
                    return;
                };
                let _ = crate::control_plane_mesh::await_until(
                    deadline,
                    client.mark_direct_validation_failure_with_operation(
                        &peer,
                        state,
                        validation_revision,
                        operation_id,
                    ),
                )
                .await;
            });
        }
    }

    pub(super) fn spawn_validation_success_cleanup(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let deadline = Instant::now() + POST_DEADLINE_CLEANUP_WAIT;
                let Some(_epoch_guard) = client.mesh_epoch_guard_until(epoch, deadline, true).await
                else {
                    return;
                };
                let _ = crate::control_plane_mesh::await_until(
                    deadline,
                    client
                        .circuits
                        .record_success_at(&peer.node_id, operation_id),
                )
                .await;
                let _ = crate::control_plane_mesh::await_until(
                    deadline,
                    client.mark_direct_validation_success_with_operation(
                        &peer,
                        validation_revision,
                        operation_id,
                    ),
                )
                .await;
            });
        }
    }

    pub(super) async fn before_public_request_until(
        &self,
        peer_id: &str,
        route: InternalRoute,
        deadline: Instant,
    ) -> Option<MeshAttemptDecision> {
        crate::control_plane_mesh::await_until(
            deadline,
            self.circuits
                .before_public_attempt_with_probe(peer_id, route == InternalRoute::HealthV2),
        )
        .await
    }
}
