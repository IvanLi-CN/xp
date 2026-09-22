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

mod circuit;
mod error;
mod gate;
mod request;
mod retry;
mod reverse;
mod telemetry;
mod transport;
pub use circuit::DirectValidationState;
use circuit::{
    DirectValidationStore, MeshAttemptDecision, PeerCircuitBreakers, endpoint_fingerprint,
    mesh_attempt_budget,
};
pub use error::MeshRequestError;
pub(crate) use request::CapabilityProbeResponse;
use request::PeerRequestResponse;
pub use request::{MeshRequest, PeerDirectPath};
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
    mesh_epoch_reset_lock: Arc<Mutex<u64>>,
    mesh_gate_lock: Arc<tokio::sync::RwLock<()>>,
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
            mesh_epoch_reset_lock: Arc::new(Mutex::new(0)),
            mesh_gate_lock: Arc::new(tokio::sync::RwLock::new(())),
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
        let cluster_mesh_enabled = self.observe_mesh_gate().await;
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
        let mesh_gate_read = self.mesh_read_guard_for_path(path).await?;
        self.send_peer_direct_request_with_gate(
            peer,
            path,
            request,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            mesh_gate_read,
        )
        .await
    }
    /// Sends over one direct path with a caller-owned Mesh admission guard. The guard may
    /// span asynchronous target preparation for dedicated probes and is never reacquired.
    pub(crate) async fn send_peer_direct_request_with_gate(
        &self,
        peer: &MeshPeerTarget,
        path: PeerDirectPath,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        mesh_gate_read: Option<tokio::sync::OwnedRwLockReadGuard<()>>,
    ) -> Result<reqwest::Response, MeshRequestError> {
        self.send_peer_direct_request_with_options(
            peer,
            path,
            request,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            mesh_gate_read,
            false,
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
        )
        .await
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
    ) -> Result<reqwest::Response, MeshRequestError> {
        let mesh_gate_read = if path == PeerDirectPath::RealityMesh && mesh_gate_read.is_none() {
            if allow_mesh_when_disabled {
                None
            } else {
                Some(self.mesh_direct_read_guard().await?)
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
        let context = RequestContext::now(
            request.route,
            request.cluster_id.clone(),
            request.sender_id.clone(),
            peer.node_id.clone(),
            request.request_id.clone(),
        );
        let client = match path {
            PeerDirectPath::RealityMesh => &self.mesh,
            PeerDirectPath::ApiBaseUrl => &self.public_direct,
        };
        let (response, verified) = retry::signed_send_with_public_gateway_retries(
            client,
            &url,
            &request,
            &context,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            request.total_budget,
            path == PeerDirectPath::ApiBaseUrl,
        )
        .await?;
        let response = match mesh_gate_read {
            Some(gate_guard) => reverse::attach_mesh_gate(response, gate_guard),
            None => response,
        };
        if path == PeerDirectPath::RealityMesh
            && mesh_transport_observation(&response).protocol != MeshTransportProtocol::H2
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
    /// Sends through Mesh first, then public only after a retryable transport failure.
    pub async fn send_peer_request(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<reqwest::Response, MeshRequestError> {
        match self
            .send_peer_request_with_legacy_not_found(
                peer,
                request,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                false,
                gate::PublicFallbackPolicy::Always,
            )
            .await?
        {
            PeerRequestResponse::Verified(response) => Ok(response),
            PeerRequestResponse::PredecessorNotFound => Err(MeshRequestError::Protocol(
                "unexpected predecessor capability response".to_string(),
            )),
        }
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
            )
            .await?;
        Ok(match response {
            PeerRequestResponse::Verified(response) => CapabilityProbeResponse::Verified(response),
            PeerRequestResponse::PredecessorNotFound => {
                CapabilityProbeResponse::PredecessorNotFound
            }
        })
    }

    async fn send_peer_request_with_legacy_not_found(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        allow_unsigned_not_found: bool,
        public_fallback_policy: gate::PublicFallbackPolicy,
    ) -> Result<PeerRequestResponse, MeshRequestError> {
        let started = Instant::now();
        let context = RequestContext::now(
            request.route,
            request.cluster_id.clone(),
            request.sender_id.clone(),
            peer.node_id.clone(),
            request.request_id.clone(),
        );
        let cluster_mesh_enabled = self.observe_mesh_gate().await;
        #[cfg(test)]
        if let Some((observed, release)) = &self.mesh_observation_pause {
            observed.notify_one();
            release.notified().await;
        }
        let mut allow_public_fallback = public_fallback_policy.allows(cluster_mesh_enabled)
            || (request.path_and_query == LEGACY_CAPABILITIES_PROBE_PATH
                && !matches!(peer.mesh_reason, MeshPeerReason::MeshAvailable));
        let (direct_validation, validation_revision, membership_read_guard) =
            self.direct_validation_snapshot(peer).await;
        let mut membership_read_guard = Some(membership_read_guard);
        if cluster_mesh_enabled
            && peer.mesh_base_url.is_some()
            && direct_validation == DirectValidationState::ProtocolRejected
        {
            self.record_terminal_failure(peer).await;
            return Err(MeshRequestError::CircuitOpen {
                path: "Direct Mesh",
            });
        }
        let mesh_enabled = peer.mesh_base_url.is_some()
            && cluster_mesh_enabled
            && matches!(
                direct_validation,
                DirectValidationState::Verified | DirectValidationState::TransportFailed
            );
        let (decision, mesh_epoch) = self
            .before_mesh_request(&peer.node_id, mesh_enabled, request.route)
            .await;
        let mut fallback = matches!(decision, MeshAttemptDecision::SkipOpen);
        let mut mesh_outcome_ambiguous = false;

        if matches!(decision, MeshAttemptDecision::Quarantined) {
            self.record_terminal_failure(peer).await;
            return Err(MeshRequestError::CircuitOpen {
                path: "Direct Mesh",
            });
        }

        if matches!(
            decision,
            MeshAttemptDecision::Attempt | MeshAttemptDecision::Probe
        ) && !self.mesh_attempt_is_current(mesh_epoch).await
        {
            fallback = true;
        }
        if matches!(
            decision,
            MeshAttemptDecision::Attempt | MeshAttemptDecision::Probe
        ) && !fallback
        {
            let mesh_url = join_url(
                peer.mesh_base_url.as_deref().expect("checked enabled"),
                &request.path_and_query,
                false,
            )?;
            let budget = mesh_attempt_budget(request.total_budget);
            match self
                .attempt_mesh_request(
                    peer,
                    &request,
                    &context,
                    &mesh_url,
                    budget,
                    mesh_epoch,
                    validation_revision.clone(),
                    membership_read_guard.take(),
                    started,
                    allow_unsigned_not_found,
                    cluster_ca_key_pem,
                    cluster_ca_cert_pem,
                )
                .await?
            {
                gate::MeshAttemptResult::Fallback { ambiguous } => {
                    fallback = true;
                    mesh_outcome_ambiguous |= ambiguous;
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
        if self.reverse_enabled.load(Ordering::Acquire)
            && should_try_reverse
            && (request.allow_ambiguous_fallback || !mesh_outcome_ambiguous)
            && let Some(reverse_route) =
                self.reverse_routes.read().await.get(&peer.node_id).cloned()
        {
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
                let mesh_epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
                match self
                    .send_reverse_relay(
                        peer,
                        &candidate,
                        &request,
                        cluster_ca_key_pem,
                        cluster_ca_cert_pem,
                        reverse_budget,
                        reverse_class,
                    )
                    .await
                {
                    Ok(response) => {
                        self.record_reverse_sample(peer, started, &request, &candidate, mesh_epoch)
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
                            && matches!(error, MeshRequestError::OutcomeUnknown)
                        {
                            self.record_terminal_failure(peer).await;
                            return Err(MeshRequestError::OutcomeUnknown);
                        }
                        if matches!(
                            error,
                            MeshRequestError::Auth(_) | MeshRequestError::Protocol(_)
                        ) {
                            self.record_terminal_failure(peer).await;
                            return Err(error);
                        }
                        // A gate rejection happens before dispatch and cannot make the outcome
                        // unknown. Transport failures remain ambiguous.
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
            allow_public_fallback = !self.observe_mesh_gate().await;
        }
        if !allow_public_fallback {
            self.record_terminal_failure(peer).await;
            return Err(if matches!(decision, MeshAttemptDecision::Disabled) {
                MeshRequestError::InvalidTarget("Mesh is unavailable".to_string())
            } else {
                MeshRequestError::OutcomeUnknown
            });
        }
        if !request.allow_ambiguous_fallback && mesh_outcome_ambiguous {
            self.record_terminal_failure(peer).await;
            return Err(MeshRequestError::OutcomeUnknown);
        }
        let elapsed = started.elapsed();
        let public_epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
        let remaining = request.total_budget.saturating_sub(elapsed);
        if remaining.is_zero() {
            self.record_terminal_failure(peer).await;
            return Err(MeshRequestError::OutcomeUnknown);
        }
        match self
            .before_public_request(&peer.node_id, request.route)
            .await
        {
            MeshAttemptDecision::SkipOpen | MeshAttemptDecision::Quarantined => {
                self.record_terminal_failure(peer).await;
                return Err(MeshRequestError::CircuitOpen { path: "Public" });
            }
            MeshAttemptDecision::Attempt
            | MeshAttemptDecision::Probe
            | MeshAttemptDecision::Disabled => {}
        }
        let public_url = join_url(&peer.public_base_url, &request.path_and_query, false)?;
        let response = match self
            .send_public_signed(
                &public_url,
                &request,
                &context,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                remaining,
                allow_unsigned_not_found,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                let public_breaker = self.circuits.record_public_failure(&peer.node_id).await;
                if let Some(telemetry) = &self.telemetry {
                    let _ = telemetry
                        .set_public_breaker(
                            &peer.node_id,
                            public_breaker,
                            Some(format!("Public circuit opened: {error}")),
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
            self.circuits.record_public_success(&peer.node_id).await;
            return Ok(PeerRequestResponse::PredecessorNotFound);
        }
        let public_breaker = self.circuits.record_public_success(&peer.node_id).await;
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .set_public_breaker(&peer.node_id, public_breaker, None)
                .await;
        }
        self.record_public_outcome_for_epoch(
            peer,
            started,
            true,
            fallback,
            request.updates_active_path,
            public_epoch,
        )
        .await;
        Ok(PeerRequestResponse::Verified(response))
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_public_signed(
        &self,
        url: &str,
        request: &MeshRequest,
        context: &RequestContext,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        budget: Duration,
        allow_unsigned_not_found: bool,
    ) -> Result<reqwest::Response, MeshRequestError> {
        let (response, verified) = retry::signed_send_with_public_gateway_retries(
            &self.public_direct,
            url,
            request,
            context,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            budget,
            true,
        )
        .await?;
        let Some(acknowledgement) = response.headers().get(internal_auth::INTERNAL_ACK_HEADER)
        else {
            if allow_unsigned_not_found && response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(response);
            }
            return Err(MeshRequestError::Protocol(
                "public response has no signed acknowledgement".to_string(),
            ));
        };
        let ack = acknowledgement.to_str().map_err(|_| {
            MeshRequestError::Protocol(
                "public response carries a malformed signed acknowledgement".to_string(),
            )
        })?;
        if let Err(error) = internal_auth::verify_ack_v2(
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            &verified,
            &context.target_id,
            response.status().as_u16(),
            ack,
        ) {
            return Err(error.into());
        }
        Ok(response)
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_reverse_relay(
        &self,
        peer: &MeshPeerTarget,
        route: &ReverseRelayRoute,
        request: &MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        budget: Duration,
        class: reverse::ReverseRequestClass,
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
        let reverse_slot = self
            .circuits
            .try_reverse_slot(&route.rendezvous.node_id, class)
            .await?;
        request
            .path_and_query
            .parse::<axum::http::Uri>()
            .map_err(|error| MeshRequestError::InvalidTarget(error.to_string()))?;
        let inner_context = RequestContext::now(
            request.route,
            request.cluster_id.clone(),
            request.sender_id.clone(),
            peer.node_id.clone(),
            request.request_id.clone(),
        );
        let (inner_headers, inner_verified) = signed_headers(
            request,
            &inner_context,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
        );
        let inner_signature = inner_headers
            .get(internal_auth::INTERNAL_SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| MeshRequestError::Reverse("inner signature is missing".to_string()))?;
        let reverse_authority = reverse::reverse_authority(route, peer);
        let mut envelope = ReverseRelayEnvelope {
            version: String::new(),
            assignment_generation: route.assignment.generation,
            target_node_id: peer.node_id.clone(),
            method: request.method.as_str().to_string(),
            uri: request.path_and_query.clone(),
            content_type: request.content_type.clone().unwrap_or_default(),
            route: request.route.as_str().to_string(),
            sender_node_id: request.sender_id.clone(),
            request_id: request.request_id.clone(),
            issued_at: inner_context.issued_at,
            content_length: request.body.len(),
            reverse_authority,
            inner_signature: inner_signature.to_string(),
            outer_signature: String::new(),
        };
        envelope.sign(cluster_ca_key_pem);

        let outer_request = MeshRequest {
            method: reqwest::Method::POST,
            path_and_query: "/api/admin/_internal/mesh/reverse-relay".to_string(),
            content_type: Some("application/octet-stream".to_string()),
            body: request.body.clone(),
            total_budget: budget,
            allow_ambiguous_fallback: request.allow_ambiguous_fallback,
            request_id: request.request_id.clone(),
            route: InternalRoute::MeshV2,
            cluster_id: request.cluster_id.clone(),
            sender_id: request.sender_id.clone(),
            updates_active_path: false,
        };
        let outer_context = RequestContext::now(
            InternalRoute::MeshV2,
            request.cluster_id.clone(),
            request.sender_id.clone(),
            route.rendezvous.node_id.clone(),
            request.request_id.clone(),
        );
        let (mut outer_headers, outer_verified) = signed_headers(
            &outer_request,
            &outer_context,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
        );
        envelope
            .insert_headers(&mut outer_headers)
            .map_err(|error| MeshRequestError::Reverse(error.to_string()))?;
        let outer_started = Instant::now();
        let local_rendezvous = self
            .local_reverse_relay
            .as_ref()
            .filter(|local| local.node_id == route.rendezvous.node_id);
        let mut response = None;
        if let Some(local) = local_rendezvous {
            let local_url = join_url(&local.base_url, &outer_request.path_and_query, false)?;
            response = Some(
                reverse::send_outer_request(
                    &self.public_direct,
                    &outer_request,
                    &local_url,
                    &outer_headers,
                    budget,
                    request.allow_ambiguous_fallback,
                    &self.cluster_mesh_enabled,
                    &self.mesh_gate_lock,
                )
                .await?,
            );
        } else if let Some(mesh_base_url) = route.rendezvous.mesh_base_url.as_deref() {
            let mesh_budget = mesh_attempt_budget(budget).min(budget);
            let mesh_url = join_url(mesh_base_url, &outer_request.path_and_query, false)?;
            match reverse::send_outer_request(
                &self.mesh,
                &outer_request,
                &mesh_url,
                &outer_headers,
                mesh_budget,
                request.allow_ambiguous_fallback,
                &self.cluster_mesh_enabled,
                &self.mesh_gate_lock,
            )
            .await
            {
                Ok(mesh_response) => response = Some(mesh_response),
                Err(MeshRequestError::OutcomeUnknown) => {
                    return Err(MeshRequestError::OutcomeUnknown);
                }
                Err(MeshRequestError::Public(_)) if !request.allow_ambiguous_fallback => {
                    return Err(MeshRequestError::OutcomeUnknown);
                }
                Err(_) => {}
            }
        }
        let response = match response {
            Some(response) => response,
            None => {
                let remaining = budget.saturating_sub(outer_started.elapsed());
                if remaining.is_zero() {
                    return Err(MeshRequestError::OutcomeUnknown);
                }
                let outer_url = join_url(
                    &route.rendezvous.public_base_url,
                    &outer_request.path_and_query,
                    false,
                )?;
                reverse::send_outer_request(
                    &self.public_direct,
                    &outer_request,
                    &outer_url,
                    &outer_headers,
                    remaining,
                    request.allow_ambiguous_fallback,
                    &self.cluster_mesh_enabled,
                    &self.mesh_gate_lock,
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

fn mesh_transport_observation(response: &reqwest::Response) -> MeshTransportObservation {
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

fn public_transport_error(
    error: reqwest::Error,
    allow_ambiguous_fallback: bool,
) -> MeshRequestError {
    if allow_ambiguous_fallback {
        MeshRequestError::Public(error)
    } else {
        MeshRequestError::OutcomeUnknown
    }
}

async fn signed_send(
    client: &reqwest::Client,
    url: &str,
    request: &MeshRequest,
    context: &RequestContext,
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
) -> Result<(reqwest::Response, internal_auth::VerifiedRequest), reqwest::Error> {
    let (headers, verified) =
        signed_headers(request, context, cluster_ca_key_pem, cluster_ca_cert_pem);
    let mut builder = client
        .request(request.method.clone(), url)
        .body(request.body.clone());
    for (name, value) in &headers {
        builder = builder.header(name, value);
    }
    let response = builder.send().await?;
    Ok((response, verified))
}

fn signed_headers(
    request: &MeshRequest,
    context: &RequestContext,
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
) -> (axum::http::HeaderMap, internal_auth::VerifiedRequest) {
    let uri = request
        .path_and_query
        .parse::<axum::http::Uri>()
        .expect("validated request path");
    let mut headers = axum::http::HeaderMap::new();
    if let Some(content_type) = request.content_type.as_deref() {
        headers.insert(
            "content-type",
            content_type.parse().expect("valid content type"),
        );
    }
    headers.insert(
        "content-length",
        request
            .body
            .len()
            .to_string()
            .parse()
            .expect("valid content length"),
    );
    // Signing failures are malformed local inputs, not network errors.
    internal_auth::sign_request_v2(
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
        &request.method,
        &uri,
        request.content_type.as_deref(),
        &request.body,
        context,
        &mut headers,
    )
    .expect("validated internal request context");
    let verified = internal_auth::verify_request_v2(
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
        &request.method,
        &uri,
        &headers,
        &request.body,
        &context.cluster_id,
        &context.target_id,
    )
    .expect("locally signed internal request verifies");
    (headers, verified)
}
#[cfg(test)]
mod mesh_fallback_tests;
#[cfg(test)]
mod mesh_gate_tests;
#[cfg(test)]
mod peer_target_edge_tests;
#[cfg(test)]
mod peer_target_tests;
#[cfg(test)]
mod retry_tests;
