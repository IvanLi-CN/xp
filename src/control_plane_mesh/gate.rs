use super::*;

pub(super) enum MeshAttemptResult {
    Fallback { ambiguous: bool },
    Response(PeerRequestResponse),
}

impl MeshAwareHttpClient {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn attempt_mesh_request(
        &self,
        peer: &MeshPeerTarget,
        request: &MeshRequest,
        context: &RequestContext,
        mesh_url: &str,
        budget: Duration,
        mesh_epoch: u64,
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
    ) -> (MeshAttemptDecision, u64) {
        let _reset_guard = self.mesh_epoch_reset_lock.lock().await;
        let epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        let decision = self.circuits.before_attempt(peer_id, enabled).await;
        (decision, epoch)
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
        if self.cluster_mesh_epoch.load(Ordering::Acquire) == epoch {
            self.circuits.release_half_open_probe(peer_id).await;
        }
    }
}

impl PeerCircuitBreakers {
    async fn clear_half_open_probes(&self) {
        let mut peers = self.peers.lock().await;
        for circuit in peers.values_mut() {
            circuit.half_open_in_flight = false;
        }
    }
}
