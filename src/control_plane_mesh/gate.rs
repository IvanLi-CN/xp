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

pub(super) fn mesh_transport_observation(response: &reqwest::Response) -> MeshTransportObservation {
    let protocol = if response.version() == reqwest::Version::HTTP_2 {
        MeshTransportProtocol::H2
    } else {
        MeshTransportProtocol::Other
    };
    let fingerprint = response
        .extensions()
        .get::<hyper_util::client::legacy::connect::HttpInfo>()
        .map(|info| MeshConnectionFingerprint {
            local_addr: info.local_addr(),
            remote_addr: info.remote_addr(),
        });
    MeshTransportObservation {
        protocol,
        fingerprint,
    }
}

impl MeshAwareHttpClient {
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
        mesh_probe_guard: &mut Option<MeshHalfOpenProbeGuard>,
        started: Instant,
        allow_unsigned_not_found: bool,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        body_lease: Option<Duration>,
    ) -> Result<MeshAttemptResult, MeshRequestError> {
        let request_deadline = started + request.total_budget;
        let mesh_send_deadline = started + budget;
        let send_result = self
            .with_mesh_send_until(mesh_epoch, mesh_send_deadline, |remaining| async move {
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
                timed_out: Instant::now() >= mesh_send_deadline,
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
                    let operation_id = self.circuits.next_operation();
                    let body_deadline = body_lease
                        .map(|lease| request_deadline + lease)
                        .unwrap_or(request_deadline);
                    let on_finish = Some(self.mesh_success_telemetry_callback(
                        peer,
                        started,
                        request,
                        transport,
                        mesh_epoch,
                        validation_revision,
                        operation_id,
                        mesh_probe_guard.take(),
                        body_deadline,
                    ));
                    let response = match body_lease {
                        Some(lease) => reverse::attach_mesh_gate_with_body_lease(
                            response,
                            gate_guard,
                            request_deadline,
                            lease,
                            on_finish,
                        ),
                        None => reverse::attach_mesh_gate_with_finish(
                            response,
                            gate_guard,
                            request_deadline,
                            on_finish,
                        ),
                    };
                    return Ok(MeshAttemptResult::Response(PeerRequestResponse::Verified(
                        response,
                    )));
                }
                if allow_unsigned_not_found && response.status() == reqwest::StatusCode::NOT_FOUND {
                    drop(response);
                    drop(gate_guard);
                    let operation_id = self.circuits.next_operation();
                    self.mesh_success_telemetry_callback(
                        peer,
                        started,
                        request,
                        transport,
                        mesh_epoch,
                        validation_revision,
                        operation_id,
                        mesh_probe_guard.take(),
                        request_deadline,
                    )(crate::mesh_gate_body::BodyFinish::Complete);
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
    fn record_mesh_success_state(
        &self,
        peer: &MeshPeerTarget,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        probe_id: Option<u64>,
        _epoch_guard: &tokio::sync::OwnedRwLockReadGuard<()>,
    ) -> Option<BreakerState> {
        if !self.mesh_gate_matches(epoch) {
            return None;
        }
        let Some(breaker_state) = self
            .circuits
            .try_record_success_at(&peer.node_id, operation_id)
        else {
            self.spawn_validation_success_cleanup(
                peer,
                epoch,
                validation_revision,
                operation_id,
                probe_id,
            );
            return None;
        };
        if !self.mesh_gate_matches(epoch) {
            return None;
        }
        let cleanup_revision = validation_revision.clone();
        if self.try_mark_direct_validation_success_with_operation(
            peer,
            validation_revision,
            operation_id,
        ) != Some(true)
        {
            self.spawn_validation_success_cleanup(
                peer,
                epoch,
                cleanup_revision,
                operation_id,
                probe_id,
            );
        }
        Some(breaker_state)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn record_mesh_success_after_body(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        updates_active_path: bool,
        transport: MeshTransportObservation,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        probe_id: Option<u64>,
        mut mesh_probe_guard: Option<MeshHalfOpenProbeGuard>,
        deadline: Instant,
    ) {
        let cleanup_revision = validation_revision.clone();
        let Some(epoch_guard) = self.mesh_epoch_guard_until(epoch, deadline, true).await else {
            self.spawn_validation_success_cleanup(
                peer,
                epoch,
                cleanup_revision,
                operation_id,
                probe_id,
            );
            return;
        };
        let breaker_state = self.record_mesh_success_state(
            peer,
            epoch,
            validation_revision,
            operation_id,
            probe_id,
            &epoch_guard,
        );
        drop(epoch_guard);
        let Some(breaker_state) = breaker_state else {
            return;
        };
        if let Some(mesh_probe_guard) = mesh_probe_guard.as_mut() {
            mesh_probe_guard.disarm();
        }
        self.set_mesh_breaker_deferred_for_epoch_until(peer, breaker_state, None, epoch, deadline)
            .await;
        self.record_sample_deferred_for_epoch_until(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                true,
                started.elapsed(),
                false,
                updates_active_path,
                Some(transport),
            ),
            epoch,
            deadline,
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn record_mesh_body_failure_after_body(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        updates_active_path: bool,
        transport: MeshTransportObservation,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        mut mesh_probe_guard: Option<MeshHalfOpenProbeGuard>,
        reason: MeshPeerReason,
        deadline: Instant,
    ) {
        let cleanup_revision = validation_revision.clone();
        let Some(epoch_guard) = self.mesh_epoch_guard_until(epoch, deadline, true).await else {
            self.spawn_retryable_failure_cleanup(peer, epoch, cleanup_revision, operation_id);
            return;
        };
        if !self.mesh_gate_matches(epoch) {
            drop(epoch_guard);
            return;
        }
        let breaker_result = super::await_until(
            deadline,
            self.circuits
                .record_retryable_failure_at(&peer.node_id, operation_id),
        )
        .await;
        drop(epoch_guard);
        let Some(breaker_state) = breaker_result.flatten() else {
            if breaker_result.is_none() {
                self.spawn_retryable_failure_cleanup(
                    peer,
                    epoch,
                    validation_revision,
                    operation_id,
                );
            }
            return;
        };
        let cleanup_revision = validation_revision.clone();
        let validation_recorded = super::await_until(
            deadline,
            self.mark_direct_validation_failure_with_operation(
                peer,
                DirectValidationState::TransportFailed,
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
                DirectValidationState::TransportFailed,
                cleanup_revision,
                operation_id,
            );
        }
        if breaker_state == BreakerState::Closed
            && let Some(guard) = mesh_probe_guard.as_mut()
        {
            guard.disarm();
        }
        self.record_sample_for_epoch_until(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                false,
                started.elapsed(),
                false,
                updates_active_path,
                Some(transport),
            ),
            epoch,
            deadline,
        )
        .await;
        self.record_mesh_reason_for_epoch_until(peer, reason, epoch, deadline)
            .await;
        self.set_mesh_breaker_for_epoch_until(
            peer,
            breaker_state,
            (breaker_state == BreakerState::Open)
                .then(|| format!("Mesh response body {reason:?} opened the breaker")),
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
        let operation_id = self.circuits.next_operation();
        let Some(breaker_state) = crate::control_plane_mesh::await_until(
            deadline,
            self.circuits
                .record_protocol_failure_at(&peer.node_id, operation_id),
        )
        .await
        .flatten() else {
            self.spawn_protocol_failure_cleanup(
                peer,
                epoch,
                validation_revision.clone(),
                operation_id,
            );
            return None;
        };
        if !self.mesh_gate_matches(epoch) {
            return None;
        }
        let cleanup_revision = validation_revision.clone();
        let recorded = crate::control_plane_mesh::await_until(
            deadline,
            self.mark_direct_validation_failure_with_operation(
                peer,
                DirectValidationState::ProtocolRejected,
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
                DirectValidationState::ProtocolRejected,
                cleanup_revision,
                operation_id,
            );
        }
        Some(breaker_state)
    }

    pub fn with_mesh_gate_lock(mut self, lock: Arc<tokio::sync::RwLock<()>>) -> Self {
        self.mesh_gate_lock = lock;
        self
    }

    pub(super) fn mesh_gate_matches(&self, epoch: u64) -> bool {
        self.cluster_mesh_enabled.load(Ordering::Acquire)
            && self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch
    }

    pub(super) fn mesh_epoch_matches(&self, epoch: u64) -> bool {
        self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch
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

    pub(super) async fn mesh_epoch_guard_until(
        &self,
        epoch: u64,
        deadline: Instant,
        require_enabled: bool,
    ) -> Option<tokio::sync::OwnedRwLockReadGuard<()>> {
        if (require_enabled && !self.cluster_mesh_enabled.load(Ordering::Acquire))
            || self.cluster_mesh_epoch.load(Ordering::Acquire) != epoch
        {
            return None;
        }
        let guard = crate::control_plane_mesh::await_until(
            deadline,
            self.mesh_epoch_barrier.clone().read_owned(),
        )
        .await?;
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
        if self.snapshot_installing.load(Ordering::Acquire) {
            return None;
        }
        let gate_lock = self.mesh_gate_lock.clone();
        let gate_guard = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            gate_lock.read_owned(),
        )
        .await
        .ok()?;
        if self.snapshot_installing.load(Ordering::Acquire)
            || !self.cluster_mesh_enabled.load(Ordering::Acquire)
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
}

impl PeerCircuitBreakers {
    pub(super) async fn clear_half_open_probes(&self) {
        let mut peers = self.peers.lock().await;
        for circuit in peers.values_mut() {
            circuit.half_open_in_flight = false;
            circuit.half_open_epoch = None;
            circuit.half_open_probe_id = None;
        }
    }
}

#[cfg(test)]
#[path = "gate_body_tests.rs"]
mod body_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn protocol_failure_cleanup_converges_after_request_deadline() {
        let client =
            MeshAwareHttpClient::new(reqwest::Client::new()).with_direct_validation_required();
        let peer = MeshPeerTarget {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            mesh_base_url: xp_test_fixtures::none(),
            endpoint_transport: None,
            endpoint_fingerprint: None,
            mesh_reason: MeshPeerReason::MissingEndpoint,
            public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
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
                .before_attempt_with_probe(xp_test_fixtures::primary_node_id(), true, false)
                .await,
            MeshAttemptDecision::Quarantined
        );
    }
    #[tokio::test]
    async fn expired_mesh_failure_schedules_breaker_cleanup() {
        let client = MeshAwareHttpClient::new(reqwest::Client::new());
        let peer = MeshPeerTarget {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            mesh_base_url: xp_test_fixtures::none(),
            endpoint_transport: None,
            endpoint_fingerprint: None,
            mesh_reason: MeshPeerReason::MissingEndpoint,
            public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
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
                    .get(xp_test_fixtures::primary_node_id())
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
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
            endpoint_transport: Some("xhttp_reality_fallback"),
            endpoint_fingerprint: Some("fingerprint".to_owned()),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
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
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
            endpoint_transport: Some("xhttp_reality_fallback"),
            endpoint_fingerprint: Some("fingerprint".to_owned()),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
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
            .find(|peer| peer.peer_id == xp_test_fixtures::primary_node_id())
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
    async fn mesh_success_telemetry_runs_after_body_releases_gate() {
        use futures_util::StreamExt;

        let temp = tempfile::tempdir().expect("telemetry directory");
        let telemetry = MeshTelemetryHandle::load(temp.path()).expect("telemetry");
        let telemetry_state = telemetry.clone().hold_state_for_test().await;
        let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
        let client = MeshAwareHttpClient::new(reqwest::Client::new())
            .with_mesh_observability(telemetry.clone())
            .with_mesh_gate_lock(gate_lock.clone());
        let peer = MeshPeerTarget {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
            endpoint_transport: Some("xhttp_reality_fallback"),
            endpoint_fingerprint: Some("fingerprint".to_owned()),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
        };
        let request = MeshRequest {
            method: reqwest::Method::GET,
            path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
            content_type: None,
            body: Vec::new(),
            total_budget: Duration::from_secs(1),
            allow_ambiguous_fallback: false,
            request_id: "telemetry-body-regression".to_owned(),
            route: InternalRoute::HealthV2,
            cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
            sender_id: "sender".to_owned(),
            updates_active_path: false,
        };
        let response = reqwest::Response::from(
            axum::http::Response::builder()
                .status(reqwest::StatusCode::OK)
                .body(reqwest::Body::from("response-body"))
                .expect("synthetic response"),
        );
        let response = super::reverse::attach_mesh_gate_with_finish(
            response,
            gate_lock.clone().read_owned().await,
            Instant::now() + Duration::from_secs(1),
            Some(client.mesh_success_telemetry_callback(
                &peer,
                Instant::now(),
                &request,
                MeshTransportObservation {
                    protocol: MeshTransportProtocol::H2,
                    fingerprint: None,
                },
                0,
                None,
                client.circuits.next_operation(),
                None,
                Instant::now() + Duration::from_secs(1),
            )),
        );
        let mut body = response.bytes_stream();
        assert!(body.next().await.expect("response data").is_ok());
        assert!(body.next().await.is_none());
        tokio::time::timeout(Duration::from_millis(100), gate_lock.write_owned())
            .await
            .expect("gate writer must not wait for persistence telemetry");
        drop(telemetry_state);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if telemetry
                    .snapshot()
                    .await
                    .peers
                    .iter()
                    .any(|peer| peer.peer_id == xp_test_fixtures::primary_node_id())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("body finish must schedule success telemetry");
    }
}
