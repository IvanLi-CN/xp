use std::{
    collections::BTreeMap,
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use tokio::sync::{Mutex, RwLock};

use crate::{
    domain::{Endpoint, Node},
    internal_auth::{self, InternalRoute, RequestContext},
    managed_default_endpoints::managed_default_vless_endpoint,
    mesh_telemetry::{
        BreakerState, MeshConnectionFingerprint, MeshPeerReason, MeshTelemetryHandle,
        MeshTelemetrySample, MeshTransportObservation, MeshTransportProtocol, TelemetryPath,
    },
    protocol::validate_reality_server_name,
    reverse_mesh::{ReverseMeshAssignment, ReverseRelayEnvelope, ReverseRole, route_budget},
};

mod admission;
mod circuit;
mod cleanup;
mod completion;
mod error;
mod gate;
#[cfg(test)]
mod mesh_body_lifecycle_tests;
mod request;
mod request_flow;
mod retry;
mod reverse;
mod telemetry;
mod transport;
pub use circuit::DirectValidationState;
use circuit::{
    DirectValidationStore, MeshAttemptDecision, MeshHalfOpenProbeGuard, PeerCircuitBreakers,
    PublicHalfOpenProbeGuard, endpoint_fingerprint, mesh_attempt_budget,
};
pub use error::MeshRequestError;
use error::{classify_mesh_failure, public_timeout};
pub(crate) use request::CapabilityProbeResponse;
use request::PeerRequestResponse;
pub use request::{MeshRequest, PeerDirectPath};
use request_flow::direct_mesh_is_eligible;
pub(super) use retry::{SignedSendError, signed_headers, signed_send};
#[cfg(test)]
pub(crate) use transport::build_mesh_http_client_with_policy;
pub(crate) use transport::build_unauthenticated_mesh_http_client;
use transport::join_url;
pub use transport::{MESH_POOL_IDLE_TIMEOUT, MeshTransportPolicy, build_mesh_http_client};
pub const MESH_FAILURES_BEFORE_OPEN: u8 = 3;
pub const MESH_BACKOFF: [Duration; 5] = [
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(240),
    Duration::from_secs(300),
];
pub const DIRECT_VALIDATION_TTL: Duration = Duration::from_secs(5 * 60);
const LEGACY_CAPABILITIES_PROBE_PATH: &str = "/api/admin/_internal/capabilities";

pub(super) async fn await_until<T>(
    deadline: Instant,
    future: impl std::future::Future<Output = T>,
) -> Option<T> {
    tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), future)
        .await
        .ok()
}

pub(super) fn body_completion_deadline(deadline: Instant) -> Instant {
    let grace_deadline = Instant::now() + Duration::from_millis(100);
    deadline.max(grace_deadline)
}

#[derive(Debug, Clone)]
pub struct MeshPeerTarget {
    pub node_id: String,
    pub node_name: String,
    pub mesh_base_url: Option<String>,
    pub endpoint_transport: Option<&'static str>,
    pub endpoint_fingerprint: Option<String>,
    pub mesh_reason: MeshPeerReason,
    pub public_base_url: String,
}
#[derive(Debug, Clone)]
pub struct ReverseRelayRoute {
    pub rendezvous: MeshPeerTarget,
    pub standby_rendezvous: Option<MeshPeerTarget>,
    pub assignment: ReverseMeshAssignment,
    pub role: ReverseRole,
}
impl ReverseRelayRoute {
    fn candidates(&self) -> Vec<Self> {
        let mut routes = vec![self.clone()];
        if let Some(rendezvous) = self.standby_rendezvous.clone() {
            let mut standby = self.clone();
            standby.rendezvous = rendezvous;
            standby.standby_rendezvous = None;
            standby.role = ReverseRole::Standby;
            routes.push(standby);
        }
        routes
    }
}
pub fn peer_target_from_node(node: &Node, endpoints: &[Endpoint]) -> MeshPeerTarget {
    let access_host = node.access_host.trim().trim_end_matches('.');
    let managed = endpoints
        .iter()
        .filter(|endpoint| endpoint.node_id == node.node_id)
        .filter(|endpoint| managed_default_vless_endpoint(endpoint).is_some())
        .collect::<Vec<_>>();
    let mesh_reason = match managed.as_slice() {
        [] => MeshPeerReason::MissingEndpoint,
        [_] if validate_reality_server_name(access_host).is_err() => {
            MeshPeerReason::InvalidAccessHost
        }
        [_] => MeshPeerReason::MeshAvailable,
        _ => MeshPeerReason::AmbiguousEndpoint,
    };
    let mesh_base_url = matches!(mesh_reason, MeshPeerReason::MeshAvailable)
        .then(|| format!("https://{access_host}:{}", managed[0].port));
    let endpoint_transport = (mesh_reason == MeshPeerReason::MeshAvailable)
        .then(|| {
            managed
                .first()
                .and_then(|endpoint| managed_default_vless_endpoint(endpoint))
        })
        .flatten()
        .map(|meta| meta.transport.mesh_label());
    let endpoint_fingerprint = (mesh_reason == MeshPeerReason::MeshAvailable)
        .then(|| endpoint_fingerprint(managed[0], access_host, endpoint_transport));
    MeshPeerTarget {
        node_id: node.node_id.clone(),
        node_name: node.node_name.clone(),
        mesh_base_url,
        endpoint_transport,
        endpoint_fingerprint,
        mesh_reason,
        public_base_url: node.api_base_url.clone(),
    }
}
#[derive(Clone)]
pub struct MeshAwareHttpClient {
    mesh: reqwest::Client,
    public_direct: reqwest::Client,
    circuits: PeerCircuitBreakers,
    direct_validation: DirectValidationStore,
    enforce_direct_validation: bool,
    cluster_mesh_enabled: Arc<AtomicBool>,
    cluster_mesh_epoch: Arc<std::sync::atomic::AtomicU64>,
    mesh_epoch_barrier: Arc<tokio::sync::RwLock<()>>,
    mesh_epoch_reset_lock: Arc<Mutex<u64>>,
    mesh_gate_lock: Arc<tokio::sync::RwLock<()>>,
    completion_dispatcher: Arc<std::sync::OnceLock<completion::CompletionDispatcher>>,
    telemetry: Option<MeshTelemetryHandle>,
    reverse_routes: Arc<RwLock<BTreeMap<String, ReverseRelayRoute>>>,
    reverse_enabled: Arc<AtomicBool>,
    local_reverse_relay: Option<reverse::LocalReverseRelay>,
    #[cfg(test)]
    mesh_observation_pause: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
}
impl MeshAwareHttpClient {
    pub fn new(direct: reqwest::Client) -> Self {
        Self::from_transport_clients(direct.clone(), direct)
    }
    pub fn from_transport_clients(mesh: reqwest::Client, public_direct: reqwest::Client) -> Self {
        Self {
            mesh,
            public_direct,
            circuits: PeerCircuitBreakers::default(),
            direct_validation: DirectValidationStore::default(),
            enforce_direct_validation: false,
            cluster_mesh_enabled: Arc::new(AtomicBool::new(true)),
            cluster_mesh_epoch: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            mesh_epoch_barrier: Arc::new(tokio::sync::RwLock::new(())),
            mesh_epoch_reset_lock: Arc::new(Mutex::new(0)),
            mesh_gate_lock: Arc::new(tokio::sync::RwLock::new(())),
            completion_dispatcher: Arc::new(std::sync::OnceLock::new()),
            telemetry: None,
            reverse_routes: Arc::new(RwLock::new(BTreeMap::new())),
            reverse_enabled: Arc::new(AtomicBool::new(cfg!(test))),
            local_reverse_relay: None,
            #[cfg(test)]
            mesh_observation_pause: None,
        }
    }
    pub fn direct(&self) -> &reqwest::Client {
        &self.public_direct
    }
    pub fn with_mesh_observability(mut self, telemetry: MeshTelemetryHandle) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    pub fn with_circuits(mut self, circuits: PeerCircuitBreakers) -> Self {
        self.circuits = circuits;
        self
    }
    pub fn with_direct_validation_required(mut self) -> Self {
        self.enforce_direct_validation = true;
        self
    }
    pub fn circuits(&self) -> PeerCircuitBreakers {
        self.circuits.clone()
    }

    pub fn with_reverse_routes(mut self, routes: BTreeMap<String, ReverseRelayRoute>) -> Self {
        self.reverse_routes = Arc::new(RwLock::new(routes));
        self
    }

    #[cfg(test)]
    pub(crate) fn with_reverse_gate(mut self, gate: Arc<AtomicBool>) -> Self {
        self.reverse_enabled = gate;
        self
    }
    #[cfg(test)]
    fn with_mesh_observation_pause(
        mut self,
        observed: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) -> Self {
        self.mesh_observation_pause = Some((observed, release));
        self
    }
    /// Attach the Raft-authoritative cluster Mesh switch. Public direct requests remain
    /// available when this gate is closed.
    pub fn with_mesh_gate(mut self, gate: Arc<AtomicBool>) -> Self {
        self.cluster_mesh_enabled = gate;
        self
    }
    pub async fn set_reverse_route(
        &self,
        target_node_id: impl Into<String>,
        route: ReverseRelayRoute,
    ) {
        self.reverse_routes
            .write()
            .await
            .insert(target_node_id.into(), route);
    }
    pub async fn clear_reverse_route(&self, target_node_id: &str) {
        self.reverse_routes.write().await.remove(target_node_id);
    }
    /// Sends over exactly one peer-direct transport. It never uses a configured proxy.
    pub async fn send_peer_direct_request(
        &self,
        peer: &MeshPeerTarget,
        path: PeerDirectPath,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<reqwest::Response, MeshRequestError> {
        let request_deadline = Instant::now() + request.total_budget;
        let cluster_mesh_enabled = self
            .observe_mesh_gate_until(request_deadline)
            .await
            .ok_or(MeshRequestError::PreDispatchTimeout)?;
        match path {
            PeerDirectPath::RealityMesh if !cluster_mesh_enabled => {
                return Err(MeshRequestError::InvalidTarget(
                    "Mesh is disabled by the cluster gate".into(),
                ));
            }
            PeerDirectPath::RealityMesh => peer.mesh_base_url.as_deref().ok_or_else(|| {
                MeshRequestError::InvalidTarget("Mesh is unavailable".to_string())
            })?,
            PeerDirectPath::ApiBaseUrl => &peer.public_base_url,
        };
        let mesh_gate_read = self
            .mesh_read_guard_for_path_until(path, request_deadline)
            .await?;
        self.send_peer_direct_request_with_gate_until(
            peer,
            path,
            request,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            mesh_gate_read,
            request_deadline,
        )
        .await
    }
    /// Sends over one direct path with a caller-owned Mesh admission guard and deadline. The
    /// guard may span asynchronous target preparation and is never reacquired.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn send_peer_direct_request_with_gate_until(
        &self,
        peer: &MeshPeerTarget,
        path: PeerDirectPath,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        mesh_gate_read: Option<tokio::sync::OwnedRwLockReadGuard<()>>,
        request_deadline: Instant,
    ) -> Result<reqwest::Response, MeshRequestError> {
        self.send_peer_direct_request_with_options(
            peer,
            path,
            request,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            mesh_gate_read,
            false,
            request_deadline,
        )
        .await
    }
    pub(crate) async fn send_peer_direct_preflight(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<reqwest::Response, MeshRequestError> {
        self.send_peer_direct_preflight_with_admission(
            peer,
            request,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            false,
        )
        .await
        .map(|(response, _)| response)
    }

    pub(crate) async fn send_peer_direct_preflight_for_reenable(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<(reqwest::Response, tokio::sync::oneshot::Receiver<bool>), MeshRequestError> {
        self.send_peer_direct_preflight_with_admission(
            peer,
            request,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            true,
        )
        .await
        .map(|(response, completion)| {
            (
                response,
                completion.expect("reenable preflight must expose completion"),
            )
        })
    }
    #[allow(clippy::too_many_arguments)]
    async fn send_peer_direct_request_with_options(
        &self,
        peer: &MeshPeerTarget,
        path: PeerDirectPath,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        mesh_gate_read: Option<tokio::sync::OwnedRwLockReadGuard<()>>,
        allow_mesh_when_disabled: bool,
        request_deadline: Instant,
    ) -> Result<reqwest::Response, MeshRequestError> {
        let mesh_gate_read = if path == PeerDirectPath::RealityMesh && mesh_gate_read.is_none() {
            if allow_mesh_when_disabled {
                None
            } else {
                Some(self.mesh_direct_read_guard_until(request_deadline).await?)
            }
        } else {
            mesh_gate_read
        };
        if path == PeerDirectPath::RealityMesh
            && !allow_mesh_when_disabled
            && !self.cluster_mesh_enabled.load(Ordering::Acquire)
        {
            return Err(MeshRequestError::InvalidTarget(
                "Mesh is disabled by the cluster gate".into(),
            ));
        }
        let base_url = match path {
            PeerDirectPath::RealityMesh => peer.mesh_base_url.as_deref().ok_or_else(|| {
                MeshRequestError::InvalidTarget("Mesh is unavailable".to_string())
            })?,
            PeerDirectPath::ApiBaseUrl => &peer.public_base_url,
        };
        let url = join_url(
            base_url,
            &request.path_and_query,
            path == PeerDirectPath::ApiBaseUrl,
        )?;
        let client = match path {
            PeerDirectPath::RealityMesh => &self.mesh,
            PeerDirectPath::ApiBaseUrl => &self.public_direct,
        };
        if request_deadline <= Instant::now() {
            return Err(MeshRequestError::PreDispatchTimeout);
        }
        let (response, verified) = retry::signed_send_with_public_gateway_retries_until(
            client,
            &url,
            &request,
            &peer.node_id,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            request_deadline,
            path == PeerDirectPath::ApiBaseUrl,
        )
        .await?;
        let response = match mesh_gate_read {
            Some(gate_guard) => reverse::attach_mesh_gate(response, gate_guard, request_deadline),
            None => response,
        };
        if path == PeerDirectPath::RealityMesh
            && gate::mesh_transport_observation(&response).protocol != MeshTransportProtocol::H2
        {
            return Err(MeshRequestError::Protocol(
                "Mesh response did not use HTTP/2".to_string(),
            ));
        }
        let ack = response
            .headers()
            .get(internal_auth::INTERNAL_ACK_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                MeshRequestError::Protocol(
                    "peer response has no signed acknowledgement".to_string(),
                )
            })?;
        internal_auth::verify_ack_v2(
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            &verified,
            &peer.node_id,
            response.status().as_u16(),
            ack,
        )?;
        Ok(response)
    }
    /// Allows a predecessor's unsigned 404 only for an explicit compatibility probe.
    pub(crate) async fn send_peer_request_allowing_legacy_not_found(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<CapabilityProbeResponse, MeshRequestError> {
        if request.method != reqwest::Method::GET
            || request.path_and_query != LEGACY_CAPABILITIES_PROBE_PATH
            || request.content_type.is_some()
            || !request.body.is_empty()
            || request.route != InternalRoute::MeshV2
        {
            return Err(MeshRequestError::Protocol(
                "legacy capability response policy is only valid for the capability probe"
                    .to_string(),
            ));
        }
        let response = self
            .send_peer_request_with_legacy_not_found(
                peer,
                request,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                true,
                gate::PublicFallbackPolicy::WhenMeshDisabled,
                None,
            )
            .await?;
        Ok(match response {
            PeerRequestResponse::Verified(response) => CapabilityProbeResponse::Verified(response),
            PeerRequestResponse::PredecessorNotFound => {
                CapabilityProbeResponse::PredecessorNotFound
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_peer_request_with_legacy_not_found(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        allow_unsigned_not_found: bool,
        public_fallback_policy: gate::PublicFallbackPolicy,
        body_lease: Option<Duration>,
    ) -> Result<PeerRequestResponse, MeshRequestError> {
        let started = Instant::now();
        let request_deadline = started + request.total_budget;
        let mesh_admission_deadline =
            started + mesh_attempt_budget(request.total_budget).min(request.total_budget);
        let observed_mesh_gate = self.observe_mesh_gate_until(mesh_admission_deadline).await;
        let admission_timed_out = observed_mesh_gate.is_none();
        let cluster_mesh_enabled = observed_mesh_gate.unwrap_or(true);
        #[cfg(test)]
        if let Some((observed, release)) = &self.mesh_observation_pause {
            observed.notify_one();
            release.notified().await;
        }
        let mut allow_public_fallback = public_fallback_policy.allows(cluster_mesh_enabled)
            || (request.path_and_query == LEGACY_CAPABILITIES_PROBE_PATH
                && !matches!(peer.mesh_reason, MeshPeerReason::MeshAvailable));
        let validation_snapshot = if admission_timed_out {
            None
        } else {
            self.direct_validation_snapshot_until(peer, mesh_admission_deadline)
                .await
        };
        let validation_snapshot_timed_out = validation_snapshot.is_none();
        let (direct_validation, validation_revision, mut membership_read_guard) =
            match validation_snapshot {
                Some((state, revision, guard)) => (state, revision, Some(guard)),
                None => (DirectValidationState::ConfiguredUnverified, None, None),
            };
        let mut admission_timed_out = admission_timed_out || validation_snapshot_timed_out;
        let admission_fallback_allowed = request.allow_ambiguous_fallback
            || matches!(request.method, reqwest::Method::GET | reqwest::Method::HEAD);
        if admission_timed_out && !admission_fallback_allowed {
            allow_public_fallback = false;
        }
        if cluster_mesh_enabled
            && peer.mesh_base_url.is_some()
            && direct_validation == DirectValidationState::ProtocolRejected
        {
            self.record_terminal_failure_until(peer, request_deadline)
                .await;
            return Err(MeshRequestError::CircuitOpen {
                path: "Direct Mesh",
                dispatched: false,
            });
        }
        let mesh_enabled = direct_mesh_is_eligible(peer, cluster_mesh_enabled, direct_validation);
        let (decision, mesh_epoch, mesh_probe_id) = match self
            .before_mesh_request_until_with_token(
                &peer.node_id,
                mesh_enabled,
                request.route,
                mesh_admission_deadline,
            )
            .await
        {
            Some(decision) => decision,
            None => {
                admission_timed_out = true;
                (
                    MeshAttemptDecision::Disabled,
                    self.cluster_mesh_epoch.load(Ordering::Acquire),
                    None,
                )
            }
        };
        if admission_timed_out && !admission_fallback_allowed {
            allow_public_fallback = false;
        }
        let mut mesh_probe_guard =
            self.mesh_probe_guard(&peer.node_id, decision, mesh_epoch, mesh_probe_id);
        let mut fallback = matches!(decision, MeshAttemptDecision::SkipOpen);
        let mut mesh_outcome_ambiguous = false;
        let mut mesh_outcome_timed_out = false;

        if matches!(decision, MeshAttemptDecision::Quarantined) {
            self.record_terminal_failure_until(peer, request_deadline)
                .await;
            return Err(MeshRequestError::CircuitOpen {
                path: "Direct Mesh",
                dispatched: false,
            });
        }

        if matches!(
            decision,
            MeshAttemptDecision::Attempt | MeshAttemptDecision::Probe
        ) && !self
            .mesh_attempt_is_current_until(mesh_epoch, mesh_admission_deadline)
            .await
        {
            if matches!(decision, MeshAttemptDecision::Probe) {
                self.release_mesh_probe_guard_until(
                    &mut mesh_probe_guard,
                    &peer.node_id,
                    mesh_epoch,
                    request_deadline,
                )
                .await;
            }
            fallback = true;
        }
        if matches!(
            decision,
            MeshAttemptDecision::Attempt | MeshAttemptDecision::Probe
        ) && !fallback
        {
            let mesh_url = match join_url(
                peer.mesh_base_url.as_deref().expect("checked enabled"),
                &request.path_and_query,
                false,
            ) {
                Ok(url) => url,
                Err(error) => {
                    if matches!(decision, MeshAttemptDecision::Probe) {
                        self.release_mesh_probe_guard_until(
                            &mut mesh_probe_guard,
                            &peer.node_id,
                            mesh_epoch,
                            request_deadline,
                        )
                        .await;
                    }
                    return Err(error);
                }
            };
            let budget = mesh_attempt_budget(request.total_budget);
            match self
                .attempt_mesh_request(
                    peer,
                    &request,
                    &mesh_url,
                    budget,
                    mesh_epoch,
                    validation_revision.clone(),
                    membership_read_guard.take(),
                    &mut mesh_probe_guard,
                    started,
                    allow_unsigned_not_found,
                    cluster_ca_key_pem,
                    cluster_ca_cert_pem,
                    body_lease,
                )
                .await?
            {
                gate::MeshAttemptResult::Fallback {
                    ambiguous,
                    timed_out,
                } => {
                    if matches!(decision, MeshAttemptDecision::Probe) {
                        self.release_mesh_probe_guard_until(
                            &mut mesh_probe_guard,
                            &peer.node_id,
                            mesh_epoch,
                            request_deadline,
                        )
                        .await;
                    }
                    fallback = true;
                    mesh_outcome_ambiguous |= ambiguous;
                    mesh_outcome_timed_out |= timed_out;
                    admission_timed_out |= timed_out && !ambiguous;
                    if timed_out && !admission_fallback_allowed {
                        allow_public_fallback = false;
                    }
                }
                gate::MeshAttemptResult::Response(response) => return Ok(response),
            }
        }

        drop(membership_read_guard);

        let should_try_reverse = cluster_mesh_enabled
            && !request.path_and_query.contains("/mesh/reverse-relay")
            && (request.path_and_query != LEGACY_CAPABILITIES_PROBE_PATH
                || matches!(peer.mesh_reason, MeshPeerReason::MeshAvailable))
            && (mesh_outcome_ambiguous
                || !mesh_enabled
                || matches!(decision, MeshAttemptDecision::SkipOpen));
        let reverse_route = if self.reverse_enabled.load(Ordering::Acquire)
            && should_try_reverse
            && (request.allow_ambiguous_fallback || !mesh_outcome_ambiguous)
        {
            match tokio::time::timeout_at(
                tokio::time::Instant::from_std(request_deadline),
                self.reverse_routes.read(),
            )
            .await
            {
                Ok(routes) => routes.get(&peer.node_id).cloned(),
                Err(_) => {
                    admission_timed_out = true;
                    None
                }
            }
        } else {
            None
        };
        if let Some(reverse_route) = reverse_route {
            // Keep one normal Mesh-sized slice for the public path. A short Raft TTL can be
            // exhausted by the failed Mesh attempt plus Reverse otherwise, leaving the known
            // reachable public origin no time to establish quorum.
            let public_fallback_reserve = if allow_public_fallback {
                mesh_attempt_budget(request.total_budget).min(request.total_budget)
            } else {
                Duration::ZERO
            };
            let reverse_class = if request.route == InternalRoute::HealthV2 {
                reverse::ReverseRequestClass::Health
            } else {
                reverse::ReverseRequestClass::Control
            };
            for candidate in reverse_route.candidates() {
                if !self.cluster_mesh_enabled.load(Ordering::Acquire) {
                    break;
                }
                let elapsed = started.elapsed();
                let remaining = request.total_budget.saturating_sub(elapsed);
                let reverse_budget = route_budget(request.total_budget)
                    .min(remaining.saturating_sub(public_fallback_reserve));
                if reverse_budget.is_zero() {
                    break;
                }
                let reverse_deadline = request_deadline.min(Instant::now() + reverse_budget);
                let mesh_epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
                match self
                    .send_reverse_relay(
                        peer,
                        &candidate,
                        &request,
                        cluster_ca_key_pem,
                        cluster_ca_cert_pem,
                        reverse_deadline,
                        reverse_class,
                        body_lease,
                    )
                    .await
                {
                    Ok(response) => {
                        self.record_reverse_sample(
                            peer,
                            started,
                            &request,
                            &candidate,
                            mesh_epoch,
                            request_deadline,
                        )
                        .await;
                        return Ok(PeerRequestResponse::Verified(response));
                    }
                    Err(error) => {
                        tracing::debug!(
                            peer_id = %peer.node_id,
                            rendezvous = %candidate.rendezvous.node_id,
                            role = ?candidate.role,
                            ?error,
                            "reverse relay attempt failed"
                        );
                        if !request.allow_ambiguous_fallback
                            && matches!(
                                error,
                                MeshRequestError::OutcomeUnknown
                                    | MeshRequestError::TransportTimeout
                                    | MeshRequestError::ReverseTimeout
                            )
                        {
                            self.record_terminal_failure_until(peer, request_deadline)
                                .await;
                            return Err(error);
                        }
                        if matches!(
                            error,
                            MeshRequestError::PreDispatchAuth(_)
                                | MeshRequestError::PreDispatchTimeout
                                | MeshRequestError::InvalidTarget(_)
                        ) {
                            return Err(error);
                        }
                        if matches!(
                            &error,
                            MeshRequestError::Reverse(reason)
                                if reason == reverse::MESH_GATE_ADMISSION_TIMEOUT
                        ) {
                            continue;
                        }
                        if matches!(
                            error,
                            MeshRequestError::Auth(_) | MeshRequestError::Protocol(_)
                        ) {
                            self.record_terminal_failure_until(peer, request_deadline)
                                .await;
                            return Err(error);
                        }
                        // A gate rejection happens before dispatch and cannot make the outcome
                        // unknown. Transport failures remain ambiguous.
                        mesh_outcome_timed_out |= matches!(
                            error,
                            MeshRequestError::TransportTimeout | MeshRequestError::ReverseTimeout
                        );
                        if !matches!(
                            error,
                            MeshRequestError::Reverse(ref reason)
                                if reason == "cluster Mesh gate is disabled"
                        ) {
                            mesh_outcome_ambiguous = true;
                        }
                    }
                }
            }
        }
        if !allow_public_fallback
            && matches!(
                public_fallback_policy,
                gate::PublicFallbackPolicy::WhenMeshDisabled
            )
        {
            // A gate transition may have happened after the initial observation while the
            // circuit decision or reverse route was waiting. Re-observe before refusing the
            // public compatibility path so a disabled gate cannot strand the probe.
            allow_public_fallback = match self.observe_mesh_gate_until(request_deadline).await {
                Some(enabled) => !enabled,
                None => {
                    admission_timed_out = true;
                    false
                }
            };
        }
        if !allow_public_fallback {
            self.record_terminal_failure_until(peer, request_deadline)
                .await;
            return Err(classify_mesh_failure(
                admission_timed_out,
                mesh_outcome_ambiguous,
                mesh_outcome_timed_out,
                decision,
            ));
        }
        if !request.allow_ambiguous_fallback && mesh_outcome_ambiguous {
            self.record_terminal_failure_until(peer, request_deadline)
                .await;
            return Err(if mesh_outcome_timed_out {
                MeshRequestError::TransportTimeout
            } else {
                MeshRequestError::OutcomeUnknown
            });
        }
        let public_epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        let Some((public_decision, public_probe_id)) = self
            .before_public_request_until(&peer.node_id, request.route, request_deadline)
            .await
        else {
            self.record_terminal_failure_until(peer, request_deadline)
                .await;
            return Err(public_timeout(
                mesh_outcome_ambiguous,
                mesh_outcome_timed_out,
                decision,
            ));
        };
        let mut public_probe_guard =
            self.public_probe_guard(&peer.node_id, public_decision, public_probe_id);
        match public_decision {
            MeshAttemptDecision::SkipOpen | MeshAttemptDecision::Quarantined => {
                self.record_terminal_failure_until(peer, request_deadline)
                    .await;
                return Err(MeshRequestError::CircuitOpen {
                    path: "Public",
                    dispatched: mesh_outcome_ambiguous,
                });
            }
            MeshAttemptDecision::Attempt
            | MeshAttemptDecision::Probe
            | MeshAttemptDecision::Disabled => {}
        }
        let public_url = match join_url(&peer.public_base_url, &request.path_and_query, false) {
            Ok(url) => url,
            Err(error) => {
                self.release_public_probe_guard_until(
                    &mut public_probe_guard,
                    &peer.node_id,
                    request_deadline,
                )
                .await;
                return Err(error);
            }
        };
        let response = match self
            .send_public_signed(
                &public_url,
                &request,
                &peer.node_id,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                request_deadline,
                allow_unsigned_not_found,
            )
            .await
        {
            Ok(response) => response,
            Err(
                error @ (MeshRequestError::PreDispatchAuth(_)
                | MeshRequestError::PreDispatchTimeout
                | MeshRequestError::InvalidTarget(_)),
            ) => {
                self.release_public_probe_guard_until(
                    &mut public_probe_guard,
                    &peer.node_id,
                    request_deadline,
                )
                .await;
                return Err(error);
            }
            Err(error) => {
                let operation_id = self.circuits.next_operation();
                let public_breaker_result = await_until(
                    request_deadline,
                    self.circuits
                        .record_public_failure_at(&peer.node_id, operation_id),
                )
                .await;
                if public_breaker_result.is_none() {
                    self.circuits
                        .spawn_public_failure_cleanup(&peer.node_id, operation_id);
                }
                if let Some(guard) = public_probe_guard.as_mut() {
                    guard.disarm();
                }
                if let Some(public_breaker) = public_breaker_result.flatten()
                    && let Some(telemetry) = &self.telemetry
                {
                    let _ = await_until(
                        request_deadline,
                        telemetry.set_public_breaker(
                            &peer.node_id,
                            public_breaker,
                            Some(format!("Public circuit opened: {error}")),
                        ),
                    )
                    .await;
                }
                self.record_public_outcome_for_epoch(
                    peer,
                    started,
                    false,
                    fallback,
                    request.updates_active_path,
                    public_epoch,
                    request_deadline,
                )
                .await;
                return Err(error);
            }
        };
        if allow_unsigned_not_found
            && response.status() == reqwest::StatusCode::NOT_FOUND
            && !response
                .headers()
                .contains_key(internal_auth::INTERNAL_ACK_HEADER)
        {
            let operation_id = self.circuits.next_operation();
            let public_breaker_result = await_until(
                request_deadline,
                self.circuits
                    .record_public_success_at(&peer.node_id, operation_id),
            )
            .await;
            if public_breaker_result.is_some()
                && let Some(guard) = public_probe_guard.as_mut()
            {
                guard.disarm();
            }
            if public_breaker_result.is_none() {
                self.circuits
                    .spawn_public_success_cleanup(&peer.node_id, operation_id);
            }
            return Ok(PeerRequestResponse::PredecessorNotFound);
        }
        let operation_id = self.circuits.next_operation();
        let body_deadline = body_lease
            .map(|lease| request_deadline + lease)
            .unwrap_or(request_deadline);
        let on_finish = Some(self.public_success_telemetry_callback(
            peer,
            started,
            fallback,
            request.updates_active_path,
            public_epoch,
            operation_id,
            public_probe_guard.take(),
            body_deadline,
        ));
        let response = match body_lease {
            Some(lease) => reverse::attach_response_with_body_lease(
                response,
                request_deadline,
                lease,
                on_finish,
            ),
            None => reverse::attach_response_with_finish(response, request_deadline, on_finish),
        };
        Ok(PeerRequestResponse::Verified(response))
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_reverse_relay(
        &self,
        peer: &MeshPeerTarget,
        route: &ReverseRelayRoute,
        request: &MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        deadline: Instant,
        class: reverse::ReverseRequestClass,
        body_lease: Option<Duration>,
    ) -> Result<reqwest::Response, MeshRequestError> {
        if !self.cluster_mesh_enabled.load(Ordering::Acquire) {
            return Err(MeshRequestError::Reverse(
                "cluster Mesh gate is disabled".to_string(),
            ));
        }
        if route.assignment.target_node_id != peer.node_id
            || !route
                .assignment
                .contains_rendezvous(&route.rendezvous.node_id)
            || route.rendezvous.node_id == peer.node_id
            || request.path_and_query.contains("/mesh/reverse-relay")
        {
            return Err(MeshRequestError::Reverse(
                "invalid reverse assignment or recursive route".to_string(),
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(MeshRequestError::PreDispatchTimeout);
        }
        let reverse_slot = self
            .circuits
            .try_reverse_slot_until(&route.rendezvous.node_id, class, deadline)
            .await?;
        let outer_request = MeshRequest {
            method: reqwest::Method::POST,
            path_and_query: "/api/admin/_internal/mesh/reverse-relay".to_string(),
            content_type: Some("application/octet-stream".to_string()),
            body: request.body.clone(),
            total_budget: remaining,
            allow_ambiguous_fallback: request.allow_ambiguous_fallback,
            request_id: request.request_id.clone(),
            route: InternalRoute::MeshV2,
            cluster_id: request.cluster_id.clone(),
            sender_id: request.sender_id.clone(),
            updates_active_path: false,
        };
        let local_rendezvous = self
            .local_reverse_relay
            .as_ref()
            .filter(|local| local.node_id == route.rendezvous.node_id);
        let mut response = None;
        if let Some(local) = local_rendezvous {
            let local_url = join_url(&local.base_url, &outer_request.path_and_query, false)?;
            let local_response = reverse::send_outer_request(
                &self.public_direct,
                peer,
                route,
                request,
                &outer_request,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                &local_url,
                deadline,
                request.allow_ambiguous_fallback,
                &self.cluster_mesh_enabled,
                &self.mesh_gate_lock,
                body_lease,
            )
            .await?;
            response = Some(local_response);
        } else if let Some(mesh_base_url) = route.rendezvous.mesh_base_url.as_deref() {
            let mesh_budget =
                mesh_attempt_budget(deadline.saturating_duration_since(Instant::now()));
            let mesh_url = join_url(mesh_base_url, &outer_request.path_and_query, false)?;
            match reverse::send_outer_request(
                &self.mesh,
                peer,
                route,
                request,
                &outer_request,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                &mesh_url,
                deadline.min(Instant::now() + mesh_budget),
                request.allow_ambiguous_fallback,
                &self.cluster_mesh_enabled,
                &self.mesh_gate_lock,
                body_lease,
            )
            .await
            {
                Ok(mesh_response) => {
                    response = Some(mesh_response);
                }
                Err(error @ MeshRequestError::TransportTimeout) => {
                    return Err(error);
                }
                Err(error @ MeshRequestError::OutcomeUnknown)
                    if !request.allow_ambiguous_fallback =>
                {
                    return Err(error);
                }
                Err(MeshRequestError::Public(_)) if !request.allow_ambiguous_fallback => {
                    return Err(MeshRequestError::OutcomeUnknown);
                }
                Err(_) => {}
            }
        }
        let (response, inner_verified, outer_verified) = match response {
            Some(response) => response,
            None => {
                if deadline <= Instant::now() {
                    return Err(MeshRequestError::OutcomeUnknown);
                }
                let outer_url = join_url(
                    &route.rendezvous.public_base_url,
                    &outer_request.path_and_query,
                    false,
                )?;
                reverse::send_outer_request(
                    &self.public_direct,
                    peer,
                    route,
                    request,
                    &outer_request,
                    cluster_ca_key_pem,
                    cluster_ca_cert_pem,
                    &outer_url,
                    deadline,
                    request.allow_ambiguous_fallback,
                    &self.cluster_mesh_enabled,
                    &self.mesh_gate_lock,
                    body_lease,
                )
                .await?
            }
        };
        reverse::verify_relay_ack(
            request,
            &response,
            &outer_verified,
            &route.rendezvous.node_id,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            internal_auth::INTERNAL_ACK_HEADER,
            "outer acknowledgement is missing",
        )?;
        reverse::verify_relay_ack(
            request,
            &response,
            &inner_verified,
            &peer.node_id,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            crate::reverse_mesh::RELAY_INNER_ACK_HEADER,
            "inner acknowledgement is missing",
        )?;
        Ok(reverse::attach_reverse_slot(response, reverse_slot))
    }
}
fn telemetry_sample(
    path: TelemetryPath,
    success: bool,
    elapsed: Duration,
    fallback: bool,
    updates_active_path: bool,
    transport: Option<MeshTransportObservation>,
) -> MeshTelemetrySample {
    MeshTelemetrySample {
        path,
        success,
        latency_ms: success.then_some(elapsed.as_millis().min(u32::MAX as u128) as u32),
        fallback,
        updates_active_path,
        transport,
    }
}

#[cfg(test)]
mod cleanup_tests;
#[cfg(test)]
mod mesh_fallback_tests;
#[cfg(test)]
mod mesh_gate_tests;
#[cfg(test)]
mod mesh_success_body_tests;
#[cfg(test)]
mod mesh_success_race_tests;
#[cfg(test)]
mod peer_target_edge_tests;
#[cfg(test)]
mod peer_target_tests;
#[cfg(test)]
mod public_body_tests;
#[cfg(test)]
mod retry_tests;
