use super::*;

const POST_DEADLINE_CLEANUP_WAIT: Duration = Duration::from_millis(100);

impl MeshAwareHttpClient {
    pub(super) fn spawn_protocol_failure_cleanup(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let Some(_epoch_guard) = client.mesh_epoch_guard_after_deadline(epoch, true).await
                else {
                    return;
                };
                client.circuits.record_protocol_failure(&peer.node_id).await;
                client
                    .mark_direct_validation_failure_at(
                        &peer,
                        DirectValidationState::ProtocolRejected,
                        validation_revision,
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
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let Some(_epoch_guard) = client.mesh_epoch_guard_after_deadline(epoch, true).await
                else {
                    return;
                };
                client
                    .circuits
                    .record_retryable_failure(&peer.node_id)
                    .await;
                client
                    .mark_direct_validation_failure_at(
                        &peer,
                        DirectValidationState::TransportFailed,
                        validation_revision,
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
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let Some(_epoch_guard) = client.mesh_epoch_guard_after_deadline(epoch, true).await
                else {
                    return;
                };
                client
                    .mark_direct_validation_failure_at(&peer, state, validation_revision)
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

    async fn mesh_epoch_guard_after_deadline(
        &self,
        epoch: u64,
        require_enabled: bool,
    ) -> Option<tokio::sync::OwnedRwLockReadGuard<()>> {
        if (require_enabled && !self.cluster_mesh_enabled.load(Ordering::Acquire))
            || self.cluster_mesh_epoch.load(Ordering::Acquire) != epoch
        {
            return None;
        }
        let guard = tokio::time::timeout(
            POST_DEADLINE_CLEANUP_WAIT,
            self.mesh_epoch_barrier.clone().read_owned(),
        )
        .await
        .ok()?;
        if (require_enabled && !self.cluster_mesh_enabled.load(Ordering::Acquire))
            || self.cluster_mesh_epoch.load(Ordering::Acquire) != epoch
        {
            return None;
        }
        Some(guard)
    }
}
