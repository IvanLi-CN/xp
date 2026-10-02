use super::*;

const POST_DEADLINE_CLEANUP_WAIT: Duration = Duration::from_millis(100);
const CLEANUP_RETRY_DELAY: Duration = Duration::from_millis(10);
const POST_DEADLINE_CLEANUP_LIFETIME: Duration = Duration::from_secs(30);

impl MeshAwareHttpClient {
    pub(super) fn spawn_protocol_failure_cleanup(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
    ) {
        self.spawn_protocol_failure_cleanup_with_requirement(
            peer,
            epoch,
            validation_revision,
            operation_id,
            true,
        );
    }

    pub(super) fn spawn_protocol_failure_cleanup_for_preflight(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        allow_mesh_when_disabled: bool,
    ) {
        self.spawn_protocol_failure_cleanup_with_requirement(
            peer,
            epoch,
            validation_revision,
            operation_id,
            !allow_mesh_when_disabled,
        );
    }

    fn spawn_protocol_failure_cleanup_with_requirement(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        require_enabled: bool,
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let cleanup_expires_at = Instant::now() + POST_DEADLINE_CLEANUP_LIFETIME;
                loop {
                    if (require_enabled && !client.mesh_gate_matches(epoch))
                        || (!require_enabled && !client.mesh_epoch_matches(epoch))
                        || Instant::now() >= cleanup_expires_at
                    {
                        return;
                    }
                    let deadline =
                        (Instant::now() + POST_DEADLINE_CLEANUP_WAIT).min(cleanup_expires_at);
                    let Some(epoch_guard) = client
                        .mesh_epoch_guard_until(epoch, deadline, require_enabled)
                        .await
                    else {
                        tokio::time::sleep(CLEANUP_RETRY_DELAY).await;
                        continue;
                    };
                    let breaker_result = crate::control_plane_mesh::await_until(
                        deadline,
                        client
                            .circuits
                            .record_protocol_failure_at(&peer.node_id, operation_id),
                    )
                    .await;
                    drop(epoch_guard);
                    match breaker_result {
                        Some(Some(_)) => break,
                        Some(None) => return,
                        None => tokio::time::sleep(CLEANUP_RETRY_DELAY).await,
                    }
                }

                loop {
                    if (require_enabled && !client.mesh_gate_matches(epoch))
                        || (!require_enabled && !client.mesh_epoch_matches(epoch))
                        || Instant::now() >= cleanup_expires_at
                    {
                        return;
                    }
                    let deadline =
                        (Instant::now() + POST_DEADLINE_CLEANUP_WAIT).min(cleanup_expires_at);
                    let Some(epoch_guard) = client
                        .mesh_epoch_guard_until(epoch, deadline, require_enabled)
                        .await
                    else {
                        tokio::time::sleep(CLEANUP_RETRY_DELAY).await;
                        continue;
                    };
                    let validation_result = crate::control_plane_mesh::await_until(
                        deadline,
                        client.mark_direct_validation_failure_with_operation(
                            &peer,
                            DirectValidationState::ProtocolRejected,
                            validation_revision.clone(),
                            operation_id,
                        ),
                    )
                    .await;
                    drop(epoch_guard);
                    match validation_result {
                        Some(_) => return,
                        None => tokio::time::sleep(CLEANUP_RETRY_DELAY).await,
                    }
                }
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
        self.spawn_retryable_failure_cleanup_with_requirement(
            peer,
            epoch,
            validation_revision,
            operation_id,
            true,
        );
    }

    pub(super) fn spawn_retryable_failure_cleanup_for_preflight(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        allow_mesh_when_disabled: bool,
    ) {
        self.spawn_retryable_failure_cleanup_with_requirement(
            peer,
            epoch,
            validation_revision,
            operation_id,
            !allow_mesh_when_disabled,
        );
    }

    fn spawn_retryable_failure_cleanup_with_requirement(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        require_enabled: bool,
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let cleanup_expires_at = Instant::now() + POST_DEADLINE_CLEANUP_LIFETIME;
                loop {
                    if (require_enabled && !client.mesh_gate_matches(epoch))
                        || (!require_enabled && !client.mesh_epoch_matches(epoch))
                        || Instant::now() >= cleanup_expires_at
                    {
                        return;
                    }
                    let deadline =
                        (Instant::now() + POST_DEADLINE_CLEANUP_WAIT).min(cleanup_expires_at);
                    let Some(epoch_guard) = client
                        .mesh_epoch_guard_until(epoch, deadline, require_enabled)
                        .await
                    else {
                        tokio::time::sleep(CLEANUP_RETRY_DELAY).await;
                        continue;
                    };
                    let breaker_result = crate::control_plane_mesh::await_until(
                        deadline,
                        client
                            .circuits
                            .record_retryable_failure_at(&peer.node_id, operation_id),
                    )
                    .await;
                    drop(epoch_guard);
                    match breaker_result {
                        Some(Some(_)) => break,
                        Some(None) => return,
                        None => tokio::time::sleep(CLEANUP_RETRY_DELAY).await,
                    }
                }

                loop {
                    if (require_enabled && !client.mesh_gate_matches(epoch))
                        || (!require_enabled && !client.mesh_epoch_matches(epoch))
                        || Instant::now() >= cleanup_expires_at
                    {
                        return;
                    }
                    let deadline =
                        (Instant::now() + POST_DEADLINE_CLEANUP_WAIT).min(cleanup_expires_at);
                    let Some(epoch_guard) = client
                        .mesh_epoch_guard_until(epoch, deadline, require_enabled)
                        .await
                    else {
                        tokio::time::sleep(CLEANUP_RETRY_DELAY).await;
                        continue;
                    };
                    let validation_result = crate::control_plane_mesh::await_until(
                        deadline,
                        client.mark_direct_validation_failure_with_operation(
                            &peer,
                            DirectValidationState::TransportFailed,
                            validation_revision.clone(),
                            operation_id,
                        ),
                    )
                    .await;
                    drop(epoch_guard);
                    match validation_result {
                        Some(_) => return,
                        None => tokio::time::sleep(CLEANUP_RETRY_DELAY).await,
                    }
                }
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
        self.spawn_validation_failure_cleanup_with_requirement(
            peer,
            epoch,
            state,
            validation_revision,
            operation_id,
            true,
        );
    }

    pub(super) fn spawn_validation_failure_cleanup_for_preflight(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        state: DirectValidationState,
        validation_revision: Option<String>,
        operation_id: u64,
        allow_mesh_when_disabled: bool,
    ) {
        self.spawn_validation_failure_cleanup_with_requirement(
            peer,
            epoch,
            state,
            validation_revision,
            operation_id,
            !allow_mesh_when_disabled,
        );
    }

    fn spawn_validation_failure_cleanup_with_requirement(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        state: DirectValidationState,
        validation_revision: Option<String>,
        operation_id: u64,
        require_enabled: bool,
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let cleanup_expires_at = Instant::now() + POST_DEADLINE_CLEANUP_LIFETIME;
                loop {
                    if (require_enabled && !client.mesh_gate_matches(epoch))
                        || (!require_enabled && !client.mesh_epoch_matches(epoch))
                        || Instant::now() >= cleanup_expires_at
                    {
                        return;
                    }
                    let deadline =
                        (Instant::now() + POST_DEADLINE_CLEANUP_WAIT).min(cleanup_expires_at);
                    let Some(epoch_guard) = client
                        .mesh_epoch_guard_until(epoch, deadline, require_enabled)
                        .await
                    else {
                        tokio::time::sleep(CLEANUP_RETRY_DELAY).await;
                        continue;
                    };
                    let validation_result = crate::control_plane_mesh::await_until(
                        deadline,
                        client.mark_direct_validation_failure_with_operation(
                            &peer,
                            state,
                            validation_revision.clone(),
                            operation_id,
                        ),
                    )
                    .await;
                    drop(epoch_guard);
                    match validation_result {
                        Some(_) => return,
                        None => tokio::time::sleep(CLEANUP_RETRY_DELAY).await,
                    }
                }
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
        self.spawn_validation_success_cleanup_with_requirement(
            peer,
            epoch,
            validation_revision,
            operation_id,
            true,
        );
    }

    pub(super) fn spawn_validation_success_cleanup_for_preflight(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        allow_mesh_when_disabled: bool,
    ) {
        self.spawn_validation_success_cleanup_with_requirement(
            peer,
            epoch,
            validation_revision,
            operation_id,
            !allow_mesh_when_disabled,
        );
    }

    fn spawn_validation_success_cleanup_with_requirement(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        require_enabled: bool,
    ) {
        let client = self.clone();
        let peer = peer.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let cleanup_expires_at = Instant::now() + POST_DEADLINE_CLEANUP_LIFETIME;
                loop {
                    if (require_enabled && !client.mesh_gate_matches(epoch))
                        || (!require_enabled && !client.mesh_epoch_matches(epoch))
                        || Instant::now() >= cleanup_expires_at
                    {
                        return;
                    }
                    let deadline =
                        (Instant::now() + POST_DEADLINE_CLEANUP_WAIT).min(cleanup_expires_at);
                    let Some(epoch_guard) = client
                        .mesh_epoch_guard_until(epoch, deadline, require_enabled)
                        .await
                    else {
                        tokio::time::sleep(CLEANUP_RETRY_DELAY).await;
                        continue;
                    };
                    let breaker_result = crate::control_plane_mesh::await_until(
                        deadline,
                        client
                            .circuits
                            .record_success_at(&peer.node_id, operation_id),
                    )
                    .await;
                    drop(epoch_guard);
                    match breaker_result {
                        Some(Some(_)) => break,
                        Some(None) => return,
                        None => tokio::time::sleep(CLEANUP_RETRY_DELAY).await,
                    }
                }

                loop {
                    if (require_enabled && !client.mesh_gate_matches(epoch))
                        || (!require_enabled && !client.mesh_epoch_matches(epoch))
                        || Instant::now() >= cleanup_expires_at
                    {
                        return;
                    }
                    let deadline =
                        (Instant::now() + POST_DEADLINE_CLEANUP_WAIT).min(cleanup_expires_at);
                    let Some(epoch_guard) = client
                        .mesh_epoch_guard_until(epoch, deadline, require_enabled)
                        .await
                    else {
                        tokio::time::sleep(CLEANUP_RETRY_DELAY).await;
                        continue;
                    };
                    let validation_result = crate::control_plane_mesh::await_until(
                        deadline,
                        client.mark_direct_validation_success_with_operation(
                            &peer,
                            validation_revision.clone(),
                            operation_id,
                        ),
                    )
                    .await;
                    drop(epoch_guard);
                    match validation_result {
                        Some(_) => return,
                        None => tokio::time::sleep(CLEANUP_RETRY_DELAY).await,
                    }
                }
            });
        }
    }

    pub(super) async fn before_public_request_until(
        &self,
        peer_id: &str,
        route: InternalRoute,
        deadline: Instant,
    ) -> Option<(MeshAttemptDecision, Option<u64>)> {
        crate::control_plane_mesh::await_until(
            deadline,
            self.circuits.before_public_attempt_with_probe_with_token(
                peer_id,
                route == InternalRoute::HealthV2,
            ),
        )
        .await
    }
}
