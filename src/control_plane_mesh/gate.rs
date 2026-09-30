use super::*;

#[derive(Clone, Copy)]
pub(super) enum PublicFallbackPolicy {
    Always,
    WhenMeshDisabled,
}

impl PublicFallbackPolicy {
    pub(super) fn allows(self, cluster_mesh_enabled: bool) -> bool {
        match self {
            Self::Always => true,
            Self::WhenMeshDisabled => !cluster_mesh_enabled,
        }
    }
}

pub(super) enum MeshAttemptResult {
    Fallback { ambiguous: bool, timed_out: bool },
    Response(PeerRequestResponse),
}

impl MeshAwareHttpClient {
    pub(crate) async fn direct_validation_snapshot(
        &self,
        peer: &MeshPeerTarget,
    ) -> (
        DirectValidationState,
        Option<String>,
        tokio::sync::OwnedRwLockReadGuard<Option<String>>,
    ) {
        let membership_guard = self.direct_validation.membership_revision_guard().await;
        let membership_revision = membership_guard.clone();
        let state = self
            .direct_validation
            .state_at(
                peer,
                self.enforce_direct_validation,
                membership_revision.as_deref(),
            )
            .await;
        (state, membership_revision, membership_guard)
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
        self.direct_validation
            .record_at(
                peer,
                DirectValidationState::Verified,
                membership_revision.as_deref(),
            )
            .await;
    }

    pub async fn mark_direct_validation_failure_at(
        &self,
        peer: &MeshPeerTarget,
        state: DirectValidationState,
        membership_revision: Option<String>,
    ) {
        self.direct_validation
            .record_at(peer, state, membership_revision.as_deref())
            .await;
    }

    pub async fn set_membership_revision(&self, revision: Option<String>) {
        self.direct_validation
            .set_membership_revision(revision)
            .await;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn attempt_mesh_request(
        &self,
        peer: &MeshPeerTarget,
        request: &MeshRequest,
        mesh_url: &str,
        budget: Duration,
        mesh_epoch: u64,
        validation_revision: Option<String>,
        _membership_guard: Option<tokio::sync::OwnedRwLockReadGuard<Option<String>>>,
        started: Instant,
        allow_unsigned_not_found: bool,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<MeshAttemptResult, MeshRequestError> {
        let request_deadline = started + request.total_budget;
        let send_result = self
            .with_mesh_send_until(mesh_epoch, started + budget, |remaining| async move {
                signed_send(
                    &self.mesh,
                    mesh_url,
                    request,
                    &peer.node_id,
                    cluster_ca_key_pem,
                    cluster_ca_cert_pem,
                    remaining,
                )
                .await
            })
            .await;
        match send_result {
            // The gate rejected admission before dispatch, so the request outcome is known.
            None => Ok(MeshAttemptResult::Fallback {
                ambiguous: false,
                timed_out: false,
            }),
            Some((Ok((response, verified)), gate_guard)) => {
                let transport = mesh_transport_observation(&response);
                if transport.protocol != MeshTransportProtocol::H2 {
                    return Err(self
                        .reject_mesh_response(
                            peer,
                            mesh_epoch,
                            response,
                            gate_guard,
                            validation_revision.clone(),
                            MeshRequestError::Protocol("Mesh response did not use HTTP/2".into()),
                            request_deadline,
                        )
                        .await);
                }
                if let Some(acknowledgement) =
                    response.headers().get(internal_auth::INTERNAL_ACK_HEADER)
                {
                    let ack = match acknowledgement.to_str() {
                        Ok(ack) => ack,
                        Err(_) => {
                            return Err(self
                                .reject_mesh_response(
                                    peer,
                                    mesh_epoch,
                                    response,
                                    gate_guard,
                                    validation_revision.clone(),
                                    MeshRequestError::Protocol(
                                        "Mesh response carries a malformed signed acknowledgement"
                                            .into(),
                                    ),
                                    request_deadline,
                                )
                                .await);
                        }
                    };
                    if let Err(error) = internal_auth::verify_ack_v2(
                        cluster_ca_key_pem,
                        cluster_ca_cert_pem,
                        &verified,
                        &peer.node_id,
                        response.status().as_u16(),
                        ack,
                    ) {
                        return Err(self
                            .reject_mesh_response(
                                peer,
                                mesh_epoch,
                                response,
                                gate_guard,
                                validation_revision.clone(),
                                error.into(),
                                request_deadline,
                            )
                            .await);
                    }
                    let breaker_state = self
                        .record_mesh_success_state(
                            peer,
                            mesh_epoch,
                            validation_revision.clone(),
                            &gate_guard,
                            request_deadline,
                        )
                        .await;
                    let response =
                        reverse::attach_mesh_gate(response, gate_guard, request_deadline);
                    if let Some(breaker_state) = breaker_state {
                        self.record_mesh_success_telemetry(
                            peer,
                            started,
                            request,
                            transport,
                            mesh_epoch,
                            breaker_state,
                            request_deadline,
                        )
                        .await;
                    }
                    return Ok(MeshAttemptResult::Response(PeerRequestResponse::Verified(
                        response,
                    )));
                }
                if allow_unsigned_not_found && response.status() == reqwest::StatusCode::NOT_FOUND {
                    let breaker_state = self
                        .record_mesh_success_state(
                            peer,
                            mesh_epoch,
                            validation_revision.clone(),
                            &gate_guard,
                            request_deadline,
                        )
                        .await;
                    drop(response);
                    drop(gate_guard);
                    if let Some(breaker_state) = breaker_state {
                        self.record_mesh_success_telemetry(
                            peer,
                            started,
                            request,
                            transport,
                            mesh_epoch,
                            breaker_state,
                            request_deadline,
                        )
                        .await;
                    }
                    return Ok(MeshAttemptResult::Response(
                        PeerRequestResponse::PredecessorNotFound,
                    ));
                }
                Err(self
                    .reject_mesh_response(
                        peer,
                        mesh_epoch,
                        response,
                        gate_guard,
                        validation_revision,
                        MeshRequestError::Protocol(
                            "Mesh response did not carry a valid signed acknowledgement".into(),
                        ),
                        request_deadline,
                    )
                    .await)
            }
            Some((Err(SignedSendError::PreDispatch(error)), gate_guard)) => {
                drop(gate_guard);
                Err(error)
            }
            Some((Err(SignedSendError::Transport(error)), gate_guard)) => {
                self.record_mesh_transport_failure(
                    peer,
                    MeshPeerReason::TransportError,
                    error.to_string(),
                    mesh_epoch,
                    gate_guard,
                    validation_revision,
                    request_deadline,
                )
                .await;
                Ok(MeshAttemptResult::Fallback {
                    ambiguous: true,
                    timed_out: error.is_timeout() && !error.is_connect(),
                })
            }
            Some((Err(SignedSendError::Timeout), gate_guard)) => {
                self.record_mesh_transport_failure(
                    peer,
                    MeshPeerReason::TransportTimeout,
                    "Mesh request timed out".into(),
                    mesh_epoch,
                    gate_guard,
                    validation_revision,
                    request_deadline,
                )
                .await;
                Ok(MeshAttemptResult::Fallback {
                    ambiguous: true,
                    timed_out: true,
                })
            }
        }
    }

    pub fn with_mesh_gate_epoch(
        mut self,
        gate: Arc<AtomicBool>,
        epoch: Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        self.cluster_mesh_enabled = gate;
        self.cluster_mesh_epoch = epoch;
        self
    }

    pub fn with_mesh_epoch_barrier(mut self, barrier: Arc<tokio::sync::RwLock<()>>) -> Self {
        self.mesh_epoch_barrier = barrier;
        self
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_mesh_success_state(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        _gate_guard: &tokio::sync::OwnedRwLockReadGuard<()>,
        deadline: Instant,
    ) -> Option<BreakerState> {
        if !self.mesh_gate_matches(epoch) {
            return None;
        }
        let breaker_state = crate::control_plane_mesh::await_until(
            deadline,
            self.circuits.record_success(&peer.node_id),
        )
        .await?;
        if !self.mesh_gate_matches(epoch) {
            return None;
        }
        let _ = crate::control_plane_mesh::await_until(
            deadline,
            self.mark_direct_validation_success_at(peer, validation_revision),
        )
        .await;
        Some(breaker_state)
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_mesh_success_telemetry(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        request: &MeshRequest,
        transport: MeshTransportObservation,
        epoch: u64,
        breaker_state: BreakerState,
        deadline: Instant,
    ) {
        if !self.mesh_gate_matches(epoch) {
            return;
        }
        self.set_mesh_breaker_for_epoch_until(peer, breaker_state, None, epoch, deadline)
            .await;
        self.record_sample_for_epoch_until(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                true,
                started.elapsed(),
                false,
                request.updates_active_path,
                Some(transport),
            ),
            epoch,
            deadline,
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn reject_mesh_response(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        response: reqwest::Response,
        gate_guard: tokio::sync::OwnedRwLockReadGuard<()>,
        validation_revision: Option<String>,
        error: MeshRequestError,
        deadline: Instant,
    ) -> MeshRequestError {
        drop(response);
        let cleanup_revision = validation_revision.clone();
        let Some(breaker_state) = self
            .record_protocol_failure_for_epoch(
                peer,
                epoch,
                validation_revision,
                &gate_guard,
                deadline,
            )
            .await
        else {
            self.spawn_protocol_failure_cleanup(peer, epoch, cleanup_revision);
            drop(gate_guard);
            return error;
        };
        drop(gate_guard);
        self.record_mesh_protocol_failure(peer, epoch, deadline)
            .await;
        self.set_mesh_breaker_for_epoch_until(
            peer,
            breaker_state,
            Some("Direct protocol rejection isolated the path".to_string()),
            epoch,
            deadline,
        )
        .await;
        self.record_terminal_failure_for_epoch(peer, epoch, deadline)
            .await;
        error
    }

    pub(super) async fn record_protocol_failure_for_epoch(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        _gate_guard: &tokio::sync::OwnedRwLockReadGuard<()>,
        deadline: Instant,
    ) -> Option<BreakerState> {
        if !self.mesh_gate_matches(epoch) {
            return None;
        }
        let breaker_state = crate::control_plane_mesh::await_until(
            deadline,
            self.circuits.record_protocol_failure(&peer.node_id),
        )
        .await?;
        if !self.mesh_gate_matches(epoch) {
            return None;
        }
        let _ = crate::control_plane_mesh::await_until(
            deadline,
            self.mark_direct_validation_failure_at(
                peer,
                DirectValidationState::ProtocolRejected,
                validation_revision,
            ),
        )
        .await;
        Some(breaker_state)
    }

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
                let Some(_epoch_guard) = client.try_mesh_epoch_guard(epoch, true) else {
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
                let Some(_epoch_guard) = client.try_mesh_epoch_guard(epoch, true) else {
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
        health_probe: bool,
    ) -> (MeshAttemptDecision, u64) {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        let epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        let decision = self
            .circuits
            .before_attempt_with_probe_at_epoch(peer_id, enabled, health_probe, Some(epoch))
            .await;
        (decision, epoch)
    }

    pub(super) async fn before_mesh_request(
        &self,
        peer_id: &str,
        enabled: bool,
        route: InternalRoute,
    ) -> (MeshAttemptDecision, u64) {
        self.before_mesh_attempt(peer_id, enabled, route == InternalRoute::HealthV2)
            .await
    }

    pub(super) async fn before_public_request(
        &self,
        peer_id: &str,
        route: InternalRoute,
    ) -> MeshAttemptDecision {
        self.circuits
            .before_public_attempt_with_probe(peer_id, route == InternalRoute::HealthV2)
            .await
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
        let (_, validation_revision, _membership_guard) =
            self.direct_validation_snapshot(peer).await;
        let (decision, epoch) = self
            .before_mesh_request(&peer.node_id, true, InternalRoute::HealthV2)
            .await;
        let mut mesh_probe_guard =
            MeshHalfOpenProbeGuard::new(&self.circuits, &peer.node_id, decision, epoch);
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
            self.release_half_open_probe_for_epoch(&peer.node_id, epoch)
                .await;
            if let Some(guard) = mesh_probe_guard.as_mut() {
                guard.disarm();
            }
        }
        if !self.mesh_epoch_is_current(epoch).await {
            return result;
        }
        if let Err(error) = &result {
            let validation_state = match error {
                MeshRequestError::PreDispatchAuth(_)
                | MeshRequestError::PreDispatchTimeout
                | MeshRequestError::InvalidTarget(_)
                | MeshRequestError::CircuitOpen { .. } => {
                    return result;
                }
                MeshRequestError::Auth(_) | MeshRequestError::Protocol(_) => {
                    DirectValidationState::ProtocolRejected
                }
                _ => DirectValidationState::TransportFailed,
            };
            match validation_state {
                DirectValidationState::ProtocolRejected => {
                    self.circuits.record_protocol_failure(&peer.node_id).await;
                }
                DirectValidationState::TransportFailed => {
                    self.circuits.record_retryable_failure(&peer.node_id).await;
                }
                _ => unreachable!("preflight failure state is classified above"),
            }
            self.mark_direct_validation_failure_at(peer, validation_state, validation_revision)
                .await;
        } else {
            self.circuits.record_success(&peer.node_id).await;
            self.mark_direct_validation_success_at(peer, validation_revision)
                .await;
        }
        result
    }

    pub(super) async fn mesh_attempt_is_current(&self, epoch: u64) -> bool {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        self.mesh_gate_matches(epoch)
    }

    pub(super) async fn mesh_epoch_is_current(&self, epoch: u64) -> bool {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch
    }

    pub(super) fn mesh_gate_matches(&self, epoch: u64) -> bool {
        self.cluster_mesh_enabled.load(Ordering::Acquire)
            && self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch
    }

    pub(super) fn try_mesh_epoch_guard(
        &self,
        epoch: u64,
        require_enabled: bool,
    ) -> Option<tokio::sync::OwnedRwLockReadGuard<()>> {
        if (require_enabled && !self.cluster_mesh_enabled.load(Ordering::Acquire))
            || self.cluster_mesh_epoch.load(Ordering::Acquire) != epoch
        {
            return None;
        }
        let guard = self.mesh_epoch_barrier.clone().try_read_owned().ok()?;
        if (require_enabled && !self.cluster_mesh_enabled.load(Ordering::Acquire))
            || self.cluster_mesh_epoch.load(Ordering::Acquire) != epoch
        {
            return None;
        }
        Some(guard)
    }

    pub(super) async fn with_mesh_send_until<T, F, Fut>(
        &self,
        epoch: u64,
        deadline: Instant,
        send: F,
    ) -> Option<(T, tokio::sync::OwnedRwLockReadGuard<()>)>
    where
        F: FnOnce(Duration) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let gate_lock = self.mesh_gate_lock.clone();
        let gate_guard = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            gate_lock.read_owned(),
        )
        .await
        .ok()?;
        if !self.cluster_mesh_enabled.load(Ordering::Acquire)
            || self.cluster_mesh_epoch.load(Ordering::Acquire) != epoch
        {
            return None;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        Some((send(remaining).await, gate_guard))
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

    pub(super) async fn release_half_open_probe_for_epoch(&self, peer_id: &str, epoch: u64) {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        self.circuits
            .release_half_open_probe_for_epoch(peer_id, epoch)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn protocol_failure_cleanup_converges_after_request_deadline() {
        let client =
            MeshAwareHttpClient::new(reqwest::Client::new()).with_direct_validation_required();
        let peer = MeshPeerTarget {
            node_id: "peer".to_owned(),
            node_name: "peer".to_owned(),
            mesh_base_url: None,
            endpoint_transport: None,
            endpoint_fingerprint: None,
            mesh_reason: MeshPeerReason::MissingEndpoint,
            public_base_url: "https://public.example".to_owned(),
        };
        let gate_guard = client.mesh_gate_lock.clone().read_owned().await;
        let response = reqwest::Response::from(
            axum::http::Response::builder()
                .status(reqwest::StatusCode::BAD_REQUEST)
                .body(reqwest::Body::from(Vec::<u8>::new()))
                .expect("synthetic response"),
        );
        let error = client
            .reject_mesh_response(
                &peer,
                0,
                response,
                gate_guard,
                None,
                MeshRequestError::Protocol("synthetic rejection".to_owned()),
                Instant::now(),
            )
            .await;
        assert!(matches!(error, MeshRequestError::Protocol(_)));
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.direct_validation_state_for(&peer).await
                    == DirectValidationState::ProtocolRejected
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("protocol rejection cleanup should converge after the request deadline");
        assert_eq!(
            client
                .circuits()
                .before_attempt_with_probe("peer", true, false)
                .await,
            MeshAttemptDecision::Quarantined
        );
    }

    #[tokio::test]
    async fn expired_mesh_failure_schedules_breaker_cleanup() {
        let client = MeshAwareHttpClient::new(reqwest::Client::new());
        let peer = MeshPeerTarget {
            node_id: "peer".to_owned(),
            node_name: "peer".to_owned(),
            mesh_base_url: None,
            endpoint_transport: None,
            endpoint_fingerprint: None,
            mesh_reason: MeshPeerReason::MissingEndpoint,
            public_base_url: "https://public.example".to_owned(),
        };
        let circuits = client.circuits();
        let peers_lock = circuits.peers.lock().await;
        let gate_guard = client.mesh_gate_lock.clone().read_owned().await;
        client
            .record_mesh_transport_failure(
                &peer,
                MeshPeerReason::TransportTimeout,
                "synthetic timeout".to_owned(),
                0,
                gate_guard,
                None,
                Instant::now(),
            )
            .await;
        drop(peers_lock);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if circuits
                    .peers
                    .lock()
                    .await
                    .get("peer")
                    .is_some_and(|circuit| circuit.failures == 1)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("expired Mesh failure should converge in the background");
    }

    #[tokio::test]
    async fn expired_mesh_failure_cleanup_skips_after_epoch_barrier_changes() {
        let client =
            MeshAwareHttpClient::new(reqwest::Client::new()).with_direct_validation_required();
        let peer = MeshPeerTarget {
            node_id: "peer".to_owned(),
            node_name: "peer".to_owned(),
            mesh_base_url: Some("https://mesh.example".to_owned()),
            endpoint_transport: Some("xhttp_reality_fallback"),
            endpoint_fingerprint: Some("fingerprint".to_owned()),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: "https://public.example".to_owned(),
        };
        client.mark_direct_validation_success_at(&peer, None).await;
        let peers_lock = client.circuits.peers.lock().await;
        let barrier_writer = client.mesh_epoch_barrier.clone().write_owned().await;
        let gate_guard = client.mesh_gate_lock.clone().read_owned().await;
        client
            .record_mesh_transport_failure(
                &peer,
                MeshPeerReason::TransportTimeout,
                "synthetic timeout".to_owned(),
                0,
                gate_guard,
                None,
                Instant::now(),
            )
            .await;
        client.cluster_mesh_epoch.store(1, Ordering::Release);
        tokio::task::yield_now().await;
        drop(barrier_writer);
        drop(peers_lock);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            client.direct_validation_state_for(&peer).await,
            DirectValidationState::Verified
        );
        assert_eq!(
            client.circuits.state(&peer.node_id, true).await,
            BreakerState::Closed
        );
    }

    #[tokio::test]
    async fn public_telemetry_records_when_mesh_is_disabled() {
        let temp = tempfile::tempdir().expect("telemetry directory");
        let telemetry = MeshTelemetryHandle::load(temp.path()).expect("telemetry");
        let client = MeshAwareHttpClient::new(reqwest::Client::new())
            .with_mesh_observability(telemetry.clone());
        client.cluster_mesh_enabled.store(false, Ordering::Release);
        let peer = MeshPeerTarget {
            node_id: "peer".to_owned(),
            node_name: "peer".to_owned(),
            mesh_base_url: Some("https://mesh.example".to_owned()),
            endpoint_transport: Some("xhttp_reality_fallback"),
            endpoint_fingerprint: Some("fingerprint".to_owned()),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: "https://public.example".to_owned(),
        };
        client
            .record_public_outcome_for_epoch(
                &peer,
                Instant::now(),
                true,
                false,
                true,
                0,
                Instant::now() + Duration::from_secs(1),
            )
            .await;
        let snapshot = telemetry.snapshot().await;
        let peer = snapshot
            .peers
            .iter()
            .find(|peer| peer.peer_id == "peer")
            .expect("disabled Mesh must still record the Public peer");
        assert_eq!(peer.last_path, Some(TelemetryPath::Public));
        assert_eq!(
            peer.buckets
                .back()
                .expect("telemetry bucket")
                .public_success,
            1
        );
    }

    #[tokio::test]
    async fn mesh_success_telemetry_does_not_requeue_gate_reader() {
        let temp = tempfile::tempdir().expect("telemetry directory");
        let telemetry = MeshTelemetryHandle::load(temp.path()).expect("telemetry");
        let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
        let client = MeshAwareHttpClient::new(reqwest::Client::new())
            .with_mesh_observability(telemetry)
            .with_mesh_gate_lock(gate_lock.clone());
        let peer = MeshPeerTarget {
            node_id: "peer".to_owned(),
            node_name: "peer".to_owned(),
            mesh_base_url: Some("https://mesh.example".to_owned()),
            endpoint_transport: Some("xhttp_reality_fallback"),
            endpoint_fingerprint: Some("fingerprint".to_owned()),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: "https://public.example".to_owned(),
        };
        let request = MeshRequest {
            method: reqwest::Method::GET,
            path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
            content_type: None,
            body: Vec::new(),
            total_budget: Duration::from_secs(1),
            allow_ambiguous_fallback: false,
            request_id: "telemetry-reader-regression".to_owned(),
            route: InternalRoute::HealthV2,
            cluster_id: "cluster".to_owned(),
            sender_id: "sender".to_owned(),
            updates_active_path: false,
        };
        let in_flight = gate_lock.clone().read_owned().await;
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let writer_lock = gate_lock.clone();
        let writer = tokio::spawn(async move {
            let _ = started_tx.send(());
            let _guard = writer_lock.write_owned().await;
            let _ = release_rx.await;
        });
        started_rx.await.expect("writer should start");
        tokio::task::yield_now().await;
        tokio::time::timeout(
            Duration::from_millis(100),
            client.record_mesh_success_telemetry(
                &peer,
                Instant::now(),
                &request,
                MeshTransportObservation {
                    protocol: MeshTransportProtocol::H2,
                    fingerprint: None,
                },
                0,
                BreakerState::Closed,
                Instant::now() + Duration::from_secs(1),
            ),
        )
        .await
        .expect("telemetry must not wait for the queued gate writer");
        drop(in_flight);
        let _ = release_tx.send(());
        writer.await.expect("writer should finish");
    }
}

impl PeerCircuitBreakers {
    async fn clear_half_open_probes(&self) {
        let mut peers = self.peers.lock().await;
        for circuit in peers.values_mut() {
            circuit.half_open_in_flight = false;
            circuit.half_open_epoch = None;
        }
    }
}
