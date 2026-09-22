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
    Fallback { ambiguous: bool },
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
        context: &RequestContext,
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
        let send_result = self
            .with_mesh_send(mesh_epoch, || async {
                tokio::time::timeout(
                    budget,
                    signed_send(
                        &self.mesh,
                        mesh_url,
                        request,
                        context,
                        cluster_ca_key_pem,
                        cluster_ca_cert_pem,
                    ),
                )
                .await
            })
            .await;
        match send_result {
            // The gate rejected admission before dispatch, so the request outcome is known.
            None => Ok(MeshAttemptResult::Fallback { ambiguous: false }),
            Some((Ok(Ok((response, verified))), gate_guard)) => {
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
                            )
                            .await);
                    }
                    self.record_mesh_success(
                        peer,
                        started,
                        request,
                        transport,
                        mesh_epoch,
                        validation_revision.clone(),
                        &gate_guard,
                    )
                    .await;
                    return Ok(MeshAttemptResult::Response(PeerRequestResponse::Verified(
                        reverse::attach_mesh_gate(response, gate_guard),
                    )));
                }
                if allow_unsigned_not_found && response.status() == reqwest::StatusCode::NOT_FOUND {
                    self.record_mesh_success(
                        peer,
                        started,
                        request,
                        transport,
                        mesh_epoch,
                        validation_revision.clone(),
                        &gate_guard,
                    )
                    .await;
                    drop(response);
                    drop(gate_guard);
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
                    )
                    .await)
            }
            Some((Ok(Err(error)), gate_guard)) => {
                drop(gate_guard);
                self.record_mesh_transport_failure(
                    peer,
                    MeshPeerReason::TransportError,
                    error.to_string(),
                    mesh_epoch,
                )
                .await;
                self.mark_direct_validation_failure_at(
                    peer,
                    DirectValidationState::TransportFailed,
                    validation_revision,
                )
                .await;
                Ok(MeshAttemptResult::Fallback { ambiguous: true })
            }
            Some((Err(_), gate_guard)) => {
                drop(gate_guard);
                self.record_mesh_transport_failure(
                    peer,
                    MeshPeerReason::TransportTimeout,
                    "Mesh request timed out".into(),
                    mesh_epoch,
                )
                .await;
                self.mark_direct_validation_failure_at(
                    peer,
                    DirectValidationState::TransportFailed,
                    validation_revision,
                )
                .await;
                Ok(MeshAttemptResult::Fallback { ambiguous: true })
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

    #[allow(clippy::too_many_arguments)]
    async fn record_mesh_success(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        request: &MeshRequest,
        transport: MeshTransportObservation,
        epoch: u64,
        validation_revision: Option<String>,
        _gate_guard: &tokio::sync::OwnedRwLockReadGuard<()>,
    ) {
        if !self.mesh_gate_matches(epoch) {
            return;
        }
        let breaker_state = self.circuits.record_success(&peer.node_id).await;
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .set_breaker(&peer.node_id, breaker_state, None)
                .await;
        }
        self.mark_direct_validation_success_at(peer, validation_revision)
            .await;
        self.record_sample(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                true,
                started.elapsed(),
                false,
                request.updates_active_path,
                Some(transport),
            ),
        )
        .await;
    }

    async fn reject_mesh_response(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        response: reqwest::Response,
        gate_guard: tokio::sync::OwnedRwLockReadGuard<()>,
        validation_revision: Option<String>,
        error: MeshRequestError,
    ) -> MeshRequestError {
        drop(response);
        self.release_half_open_probe_for_epoch(&peer.node_id, epoch)
            .await;
        let Some(breaker_state) = self
            .record_protocol_failure_for_epoch(peer, epoch, validation_revision, &gate_guard)
            .await
        else {
            drop(gate_guard);
            return error;
        };
        drop(gate_guard);
        self.record_mesh_protocol_failure(peer, epoch).await;
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .set_breaker(
                    &peer.node_id,
                    breaker_state,
                    Some("Direct protocol rejection isolated the path".to_string()),
                )
                .await;
        }
        self.record_terminal_failure_for_epoch(peer, epoch).await;
        error
    }

    pub(super) async fn record_protocol_failure_for_epoch(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        _gate_guard: &tokio::sync::OwnedRwLockReadGuard<()>,
    ) -> Option<BreakerState> {
        if !self.mesh_gate_matches(epoch) {
            return None;
        }
        let breaker_state = self.circuits.record_protocol_failure(&peer.node_id).await;
        self.mark_direct_validation_failure_at(
            peer,
            DirectValidationState::ProtocolRejected,
            validation_revision,
        )
        .await;
        Some(breaker_state)
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
            .before_attempt_with_probe(peer_id, enabled, health_probe)
            .await;
        if matches!(decision, MeshAttemptDecision::Probe) {
            self.circuits
                .mark_half_open_probe_epoch(peer_id, epoch)
                .await;
        }
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
        let (_, validation_revision, _membership_guard) =
            self.direct_validation_snapshot(peer).await;
        let (decision, epoch) = self
            .before_mesh_request(&peer.node_id, true, InternalRoute::HealthV2)
            .await;
        if matches!(
            decision,
            MeshAttemptDecision::SkipOpen | MeshAttemptDecision::Quarantined
        ) {
            return Err(MeshRequestError::CircuitOpen {
                path: "Direct Mesh",
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
            )
            .await;
        if matches!(decision, MeshAttemptDecision::Probe) {
            self.release_half_open_probe_for_epoch(&peer.node_id, epoch)
                .await;
        }
        if let Err(error) = &result {
            let validation_state = match error {
                MeshRequestError::Auth(_) | MeshRequestError::Protocol(_) => {
                    DirectValidationState::ProtocolRejected
                }
                MeshRequestError::InvalidTarget(_) | MeshRequestError::CircuitOpen { .. } => {
                    return result;
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
        }
        result
    }

    pub(super) async fn mesh_attempt_is_current(&self, epoch: u64) -> bool {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        self.mesh_gate_matches(epoch)
    }

    pub(super) fn mesh_gate_matches(&self, epoch: u64) -> bool {
        self.cluster_mesh_enabled.load(Ordering::Acquire)
            && self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch
    }

    pub(super) async fn with_mesh_send<T, F, Fut>(
        &self,
        epoch: u64,
        send: F,
    ) -> Option<(T, tokio::sync::OwnedRwLockReadGuard<()>)>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let gate_lock = self.mesh_gate_lock.clone();
        let gate_guard = gate_lock.read_owned().await;
        if !self.cluster_mesh_enabled.load(Ordering::Acquire)
            || self.cluster_mesh_epoch.load(Ordering::Acquire) != epoch
        {
            return None;
        }
        Some((send().await, gate_guard))
    }

    pub(super) async fn mesh_read_guard_for_epoch(
        &self,
        epoch: u64,
    ) -> Option<tokio::sync::OwnedRwLockReadGuard<()>> {
        let guard = self.mesh_gate_lock.clone().read_owned().await;
        (self.cluster_mesh_enabled.load(Ordering::Acquire)
            && self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch)
            .then_some(guard)
    }

    pub(super) async fn mesh_direct_read_guard(
        &self,
    ) -> Result<tokio::sync::OwnedRwLockReadGuard<()>, MeshRequestError> {
        let epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        self.mesh_read_guard_for_epoch(epoch).await.ok_or_else(|| {
            MeshRequestError::InvalidTarget("Mesh is disabled by the cluster gate".into())
        })
    }

    pub(super) async fn mesh_read_guard_for_path(
        &self,
        path: PeerDirectPath,
    ) -> Result<Option<tokio::sync::OwnedRwLockReadGuard<()>>, MeshRequestError> {
        if path == PeerDirectPath::RealityMesh {
            self.mesh_direct_read_guard().await.map(Some)
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

impl PeerCircuitBreakers {
    async fn clear_half_open_probes(&self) {
        let mut peers = self.peers.lock().await;
        for circuit in peers.values_mut() {
            circuit.half_open_in_flight = false;
            circuit.half_open_epoch = None;
        }
    }
}
