use super::*;

impl MeshAwareHttpClient {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn mesh_success_telemetry_callback(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        request: &MeshRequest,
        transport: MeshTransportObservation,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        mesh_probe_guard: Option<MeshHalfOpenProbeGuard>,
        deadline: Instant,
    ) -> Box<dyn FnOnce(crate::mesh_gate_body::BodyFinish) + Send + 'static> {
        self.mesh_success_telemetry_callback_inner(
            peer,
            started,
            request,
            transport,
            epoch,
            validation_revision,
            operation_id,
            mesh_probe_guard,
            deadline,
            None,
        )
    }

    #[cfg(test)]
    pub(super) fn mesh_success_telemetry_callback_with_completion(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        request: &MeshRequest,
        transport: MeshTransportObservation,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        mesh_probe_guard: Option<MeshHalfOpenProbeGuard>,
        deadline: Instant,
        completion: tokio::sync::oneshot::Sender<()>,
    ) -> Box<dyn FnOnce(crate::mesh_gate_body::BodyFinish) + Send + 'static> {
        self.mesh_success_telemetry_callback_inner(
            peer,
            started,
            request,
            transport,
            epoch,
            validation_revision,
            operation_id,
            mesh_probe_guard,
            deadline,
            Some(completion),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn mesh_success_telemetry_callback_inner(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        request: &MeshRequest,
        transport: MeshTransportObservation,
        epoch: u64,
        validation_revision: Option<String>,
        operation_id: u64,
        mesh_probe_guard: Option<MeshHalfOpenProbeGuard>,
        deadline: Instant,
        completion: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> Box<dyn FnOnce(crate::mesh_gate_body::BodyFinish) + Send + 'static> {
        let client = self.clone();
        let peer = peer.clone();
        let updates_active_path = request.updates_active_path;
        let probe_id = mesh_probe_guard
            .as_ref()
            .map(MeshHalfOpenProbeGuard::probe_id);
        let mut completion = completion;
        Box::new(move |outcome| {
            let completion_client = client.clone();
            let key = format!("mesh:{}", peer.node_id);
            match outcome {
                crate::mesh_gate_body::BodyFinish::Complete => {
                    let completion_deadline = super::body_completion_deadline(deadline);
                    let finished = completion.take();
                    client.dispatch_ordered_critical_completion(key, operation_id, async move {
                        completion_client
                            .record_mesh_success_after_body(
                                &peer,
                                started,
                                updates_active_path,
                                transport,
                                epoch,
                                validation_revision,
                                operation_id,
                                probe_id,
                                mesh_probe_guard,
                                completion_deadline,
                            )
                            .await;
                        if let Some(finished) = finished {
                            let _ = finished.send(());
                        }
                    });
                }
                crate::mesh_gate_body::BodyFinish::Error
                | crate::mesh_gate_body::BodyFinish::Cancelled => {
                    let failure_deadline = super::body_completion_deadline(deadline);
                    let finished = completion.take();
                    client.dispatch_ordered_critical_completion(key, operation_id, async move {
                        completion_client
                            .record_mesh_body_failure_after_body(
                                &peer,
                                started,
                                updates_active_path,
                                transport,
                                epoch,
                                validation_revision,
                                operation_id,
                                mesh_probe_guard,
                                MeshPeerReason::TransportError,
                                failure_deadline,
                            )
                            .await;
                        if let Some(finished) = finished {
                            let _ = finished.send(());
                        }
                    });
                }
                crate::mesh_gate_body::BodyFinish::Deadline
                | crate::mesh_gate_body::BodyFinish::LeaseExpired => {
                    let failure_deadline = super::body_completion_deadline(deadline);
                    let finished = completion.take();
                    client.dispatch_ordered_critical_completion(key, operation_id, async move {
                        completion_client
                            .record_mesh_body_failure_after_body(
                                &peer,
                                started,
                                updates_active_path,
                                transport,
                                epoch,
                                validation_revision,
                                operation_id,
                                mesh_probe_guard,
                                MeshPeerReason::TransportTimeout,
                                failure_deadline,
                            )
                            .await;
                        if let Some(finished) = finished {
                            let _ = finished.send(());
                        }
                    });
                }
            }
        })
    }

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
        probe_id: Option<u64>,
    ) {
        if !self.mesh_gate_matches(epoch) {
            drop(gate_guard);
            return;
        }
        let operation_id = self.circuits.next_operation();
        let breaker_result = super::await_until(
            deadline,
            self.circuits.record_retryable_failure_at_with_cleanup(
                &peer.node_id,
                operation_id,
                Some(cleanup::DirectCleanupContext::new(deadline, probe_id)),
            ),
        )
        .await;
        let Some(state) = breaker_result.flatten() else {
            if breaker_result.is_none() {
                self.spawn_retryable_failure_cleanup(
                    peer,
                    epoch,
                    validation_revision,
                    operation_id,
                    probe_id,
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
            self.mark_direct_validation_failure_with_operation_until(
                peer,
                DirectValidationState::TransportFailed,
                validation_revision,
                operation_id,
                deadline,
            ),
        )
        .await
        .flatten()
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

    pub(super) async fn record_mesh_reason_for_epoch_until(
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
                telemetry.set_mesh_reason_until(
                    &peer.node_id,
                    peer.mesh_base_url.as_deref(),
                    reason,
                    deadline,
                ),
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
                telemetry.record_sample_until(&peer.node_id, &peer.node_name, sample, deadline),
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
            telemetry.set_breaker_until(&peer.node_id, state, event_message, deadline),
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
            let completion_client = client.clone();
            let key = format!("public:{}", peer.node_id);
            match outcome {
                crate::mesh_gate_body::BodyFinish::Complete => {
                    let completion_deadline = super::body_completion_deadline(deadline);
                    client.dispatch_ordered_critical_completion(key, operation_id, async move {
                        completion_client
                            .record_public_success_after_body(
                                &peer,
                                started,
                                fallback,
                                updates_active_path,
                                public_epoch,
                                operation_id,
                                public_probe_guard,
                                completion_deadline,
                            )
                            .await;
                    });
                }
                crate::mesh_gate_body::BodyFinish::Error
                | crate::mesh_gate_body::BodyFinish::Cancelled
                | crate::mesh_gate_body::BodyFinish::Deadline
                | crate::mesh_gate_body::BodyFinish::LeaseExpired => {
                    let reason = match outcome {
                        crate::mesh_gate_body::BodyFinish::Deadline
                        | crate::mesh_gate_body::BodyFinish::LeaseExpired => {
                            MeshPeerReason::TransportTimeout
                        }
                        crate::mesh_gate_body::BodyFinish::Error
                        | crate::mesh_gate_body::BodyFinish::Cancelled => {
                            MeshPeerReason::TransportError
                        }
                        crate::mesh_gate_body::BodyFinish::Complete => unreachable!(),
                    };
                    let failure_deadline = super::body_completion_deadline(deadline);
                    client.dispatch_ordered_critical_completion(key, operation_id, async move {
                        completion_client
                            .record_public_body_failure_after_body(
                                &peer,
                                started,
                                fallback,
                                updates_active_path,
                                public_epoch,
                                operation_id,
                                public_probe_guard,
                                reason,
                                failure_deadline,
                            )
                            .await;
                    });
                }
            }
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
        let probe_id = public_probe_guard
            .as_ref()
            .map(PublicHalfOpenProbeGuard::probe_id);
        let public_breaker_result = super::await_until(
            deadline,
            self.circuits.record_public_success_at_with_cleanup(
                &peer.node_id,
                operation_id,
                Some(cleanup::PublicCleanupContext::new(deadline, probe_id)),
            ),
        )
        .await;
        if public_breaker_result.is_none() {
            self.defer_public_success(&peer.node_id, operation_id, probe_id);
        }
        let Some(public_breaker) = public_breaker_result.flatten() else {
            return;
        };
        if let Some(guard) = public_probe_guard.as_mut() {
            guard.disarm();
        }
        if let Some(telemetry) = &self.telemetry {
            let _ = super::await_until(
                deadline,
                telemetry.set_public_breaker_until(&peer.node_id, public_breaker, None, deadline),
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

    #[allow(clippy::too_many_arguments)]
    async fn record_public_body_failure_after_body(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        fallback: bool,
        updates_active_path: bool,
        public_epoch: u64,
        operation_id: u64,
        mut public_probe_guard: Option<PublicHalfOpenProbeGuard>,
        reason: MeshPeerReason,
        deadline: Instant,
    ) {
        let probe_id = public_probe_guard
            .as_ref()
            .map(PublicHalfOpenProbeGuard::probe_id);
        let public_breaker_result = super::await_until(
            deadline,
            self.circuits.record_public_failure_at_with_cleanup(
                &peer.node_id,
                operation_id,
                Some(cleanup::PublicCleanupContext::new(deadline, probe_id)),
            ),
        )
        .await;
        if public_breaker_result.is_none() {
            self.defer_public_failure(&peer.node_id, operation_id, probe_id);
        }
        let Some(public_breaker) = public_breaker_result.flatten() else {
            return;
        };
        if let Some(guard) = public_probe_guard.as_mut() {
            guard.disarm();
        }
        if let Some(telemetry) = &self.telemetry {
            let _ = super::await_until(
                deadline,
                telemetry.set_public_breaker_until(
                    &peer.node_id,
                    public_breaker,
                    (public_breaker == BreakerState::Open)
                        .then(|| format!("Public response body {reason:?}")),
                    deadline,
                ),
            )
            .await;
        }
        self.record_public_outcome_for_epoch(
            peer,
            started,
            false,
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
                telemetry.record_terminal_failure_until(&peer.node_id, &peer.node_name, deadline),
            )
            .await;
        }
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;

    fn peer() -> MeshPeerTarget {
        MeshPeerTarget {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
            endpoint_transport: Some("xhttp_reality_fallback"),
            endpoint_fingerprint: Some("fingerprint".to_owned()),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
        }
    }

    #[tokio::test]
    async fn expired_public_body_success_does_not_close_the_breaker() {
        let client = MeshAwareHttpClient::new(reqwest::Client::new());
        let peer = peer();
        for _ in 0..MESH_FAILURES_BEFORE_OPEN {
            let operation_id = client.circuits.next_operation();
            client
                .circuits
                .record_public_failure_at(&peer.node_id, operation_id)
                .await;
        }
        assert_eq!(
            client.circuits.public_state(&peer.node_id).await,
            BreakerState::Open
        );

        client
            .record_public_success_after_body(
                &peer,
                Instant::now(),
                false,
                false,
                0,
                client.circuits.next_operation(),
                None,
                Instant::now() - Duration::from_millis(1),
            )
            .await;

        assert_eq!(
            client.circuits.public_state(&peer.node_id).await,
            BreakerState::Open,
            "an expired callback must not commit after acquiring the circuit lock"
        );
    }

    #[tokio::test]
    async fn expired_public_body_failure_does_not_extend_the_cooldown() {
        let client = MeshAwareHttpClient::new(reqwest::Client::new());
        let peer = peer();
        let operation_id = client.circuits.next_operation();
        client
            .circuits
            .record_public_failure_at(&peer.node_id, operation_id)
            .await;
        let before = client
            .circuits
            .public_failure_snapshot_for_test(&peer.node_id)
            .await;
        assert_eq!(
            client.circuits.public_state(&peer.node_id).await,
            BreakerState::Open
        );

        client
            .record_public_body_failure_after_body(
                &peer,
                Instant::now(),
                false,
                false,
                0,
                client.circuits.next_operation(),
                None,
                MeshPeerReason::TransportError,
                Instant::now() - Duration::from_millis(1),
            )
            .await;

        assert_eq!(
            client
                .circuits
                .public_failure_snapshot_for_test(&peer.node_id)
                .await,
            before,
            "an expired callback must not commit after acquiring the circuit lock"
        );
    }

    #[tokio::test]
    async fn expired_mesh_telemetry_does_not_write_a_route_reason() {
        let temp = tempfile::tempdir().expect("telemetry directory");
        let telemetry = MeshTelemetryHandle::load(temp.path()).expect("telemetry");
        let client = MeshAwareHttpClient::new(reqwest::Client::new())
            .with_mesh_observability(telemetry.clone());
        let peer = peer();

        client
            .record_mesh_reason_until(
                &peer,
                MeshPeerReason::TransportTimeout,
                Instant::now() - Duration::from_millis(1),
            )
            .await;

        assert!(telemetry.snapshot().await.peers.is_empty());
    }

    #[tokio::test]
    async fn completed_mesh_body_persists_success_breaker_and_reason() {
        let temp = tempfile::tempdir().expect("telemetry directory");
        let telemetry = MeshTelemetryHandle::load(temp.path()).expect("telemetry");
        let peer = peer();
        telemetry
            .set_breaker(&peer.node_id, BreakerState::Open, None)
            .await
            .expect("seed breaker");
        telemetry
            .set_mesh_reason(
                &peer.node_id,
                peer.mesh_base_url.as_deref(),
                MeshPeerReason::TransportError,
            )
            .await
            .expect("seed route reason");
        let client =
            MeshAwareHttpClient::new(reqwest::Client::new()).with_mesh_observability(telemetry);

        client
            .record_mesh_success_after_body(
                &peer,
                Instant::now(),
                false,
                MeshTransportObservation {
                    protocol: MeshTransportProtocol::H2,
                    fingerprint: None,
                },
                0,
                None,
                client.circuits.next_operation(),
                None,
                None,
                Instant::now() + Duration::from_secs(1),
            )
            .await;

        let restored = MeshTelemetryHandle::load(temp.path()).expect("restored telemetry");
        let snapshot = restored.snapshot().await;
        let persisted_peer = snapshot
            .peers
            .iter()
            .find(|item| item.peer_id == peer.node_id)
            .expect("persisted peer");
        assert_eq!(persisted_peer.breaker, Some(BreakerState::Closed));
        assert_eq!(
            persisted_peer.last_mesh_reason,
            Some(MeshPeerReason::MeshAvailable)
        );
    }
}
