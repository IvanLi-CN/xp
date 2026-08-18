use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

use crate::{
    domain::{Endpoint, Node},
    internal_auth::{self, InternalRoute, RequestContext},
    managed_default_endpoints::managed_default_vless_endpoint,
    mesh_telemetry::{
        BreakerState, MeshConnectionFingerprint, MeshPeerReason, MeshTelemetryHandle,
        MeshTelemetrySample, MeshTransportObservation, MeshTransportProtocol, TelemetryPath,
    },
    protocol::validate_reality_server_name,
};

mod transport;
#[cfg(test)]
pub(crate) use transport::build_mesh_http_client_with_policy;
pub(crate) use transport::build_unauthenticated_mesh_http_client;
pub use transport::{MESH_POOL_IDLE_TIMEOUT, MeshTransportPolicy, build_mesh_http_client};

pub const MESH_FAILURES_BEFORE_OPEN: u8 = 3;
pub const MESH_BACKOFF: [Duration; 5] = [
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(240),
    Duration::from_secs(300),
];
const LEGACY_CAPABILITIES_PROBE_PATH: &str = "/api/admin/_internal/capabilities";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshAttemptDecision {
    Attempt,
    Probe,
    SkipOpen,
    Disabled,
}

#[derive(Debug, Clone, Default)]
struct PeerCircuit {
    failures: u8,
    open_count: usize,
    retry_at: Option<Instant>,
    half_open_in_flight: bool,
}

#[derive(Clone, Default)]
pub struct PeerCircuitBreakers {
    peers: Arc<Mutex<BTreeMap<String, PeerCircuit>>>,
}

impl PeerCircuitBreakers {
    pub async fn before_attempt(&self, peer_id: &str, enabled: bool) -> MeshAttemptDecision {
        if !enabled {
            return MeshAttemptDecision::Disabled;
        }
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        match circuit.retry_at {
            None => MeshAttemptDecision::Attempt,
            Some(retry_at) if now < retry_at => MeshAttemptDecision::SkipOpen,
            Some(_) if circuit.half_open_in_flight => MeshAttemptDecision::SkipOpen,
            Some(_) => {
                circuit.half_open_in_flight = true;
                MeshAttemptDecision::Probe
            }
        }
    }
    pub async fn record_success(&self, peer_id: &str) -> BreakerState {
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        circuit.failures = 0;
        circuit.open_count = 0;
        circuit.retry_at = None;
        circuit.half_open_in_flight = false;
        BreakerState::Closed
    }
    pub async fn release_half_open_probe(&self, peer_id: &str) {
        let mut peers = self.peers.lock().await;
        if let Some(circuit) = peers.get_mut(peer_id) {
            circuit.half_open_in_flight = false;
        }
    }
    /// Only retryable transport errors may alter the breaker.
    pub async fn record_retryable_failure(&self, peer_id: &str) -> BreakerState {
        let now = Instant::now();
        let mut peers = self.peers.lock().await;
        let circuit = peers.entry(peer_id.to_string()).or_default();
        circuit.half_open_in_flight = false;
        circuit.failures = circuit.failures.saturating_add(1);
        if circuit.failures < MESH_FAILURES_BEFORE_OPEN {
            return BreakerState::Closed;
        }
        let backoff = MESH_BACKOFF[circuit.open_count.min(MESH_BACKOFF.len() - 1)];
        circuit.open_count = circuit.open_count.saturating_add(1);
        circuit.retry_at = Some(now + backoff);
        BreakerState::Open
    }
    pub async fn state(&self, peer_id: &str, enabled: bool) -> BreakerState {
        if !enabled {
            return BreakerState::Disabled;
        }
        let peers = self.peers.lock().await;
        let Some(circuit) = peers.get(peer_id) else {
            return BreakerState::Closed;
        };
        match circuit.retry_at {
            None => BreakerState::Closed,
            Some(retry_at) if Instant::now() < retry_at => BreakerState::Open,
            Some(_) => BreakerState::HalfOpen,
        }
    }
}
pub fn mesh_attempt_budget(total: Duration) -> Duration {
    let third = total / 3;
    third.clamp(Duration::from_millis(500), Duration::from_secs(5))
}
#[derive(Debug, Clone)]
pub struct MeshPeerTarget {
    pub node_id: String,
    pub node_name: String,
    pub mesh_base_url: Option<String>,
    pub mesh_reason: MeshPeerReason,
    pub public_base_url: String,
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
    MeshPeerTarget {
        node_id: node.node_id.clone(),
        node_name: node.node_name.clone(),
        mesh_base_url,
        mesh_reason,
        public_base_url: node.api_base_url.clone(),
    }
}
#[derive(Debug, Clone)]
pub struct MeshRequest {
    pub method: reqwest::Method,
    pub path_and_query: String,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub total_budget: Duration,
    pub allow_ambiguous_fallback: bool,
    pub request_id: String,
    pub route: InternalRoute,
    pub cluster_id: String,
    pub sender_id: String,
    pub updates_active_path: bool,
}

/// The only compatibility result accepted from the signed capability probe.
pub(crate) enum CapabilityProbeResponse {
    Verified(reqwest::Response),
    PredecessorNotFound,
}

enum PeerRequestResponse {
    Verified(reqwest::Response),
    PredecessorNotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerDirectPath {
    RealityMesh,
    ApiBaseUrl,
}
#[derive(Debug)]
pub enum MeshRequestError {
    InvalidTarget(String),
    Auth(internal_auth::AuthError),
    OutcomeUnknown,
    Protocol(String),
    Public(reqwest::Error),
}
impl From<internal_auth::AuthError> for MeshRequestError {
    fn from(value: internal_auth::AuthError) -> Self {
        Self::Auth(value)
    }
}
impl std::fmt::Display for MeshRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTarget(value) => write!(f, "invalid Mesh peer target: {value}"),
            Self::Auth(value) => write!(f, "internal authentication error: {value}"),
            Self::OutcomeUnknown => {
                f.write_str("Mesh request outcome is unknown; it may already have been applied")
            }
            Self::Protocol(value) => write!(f, "Mesh protocol error: {value}"),
            Self::Public(value) => write!(f, "public fallback failed: {value}"),
        }
    }
}
impl std::error::Error for MeshRequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Auth(value) => Some(value),
            Self::Public(value) => Some(value),
            _ => None,
        }
    }
}
#[derive(Clone)]
pub struct MeshAwareHttpClient {
    mesh: reqwest::Client,
    public_direct: reqwest::Client,
    circuits: PeerCircuitBreakers,
    telemetry: Option<MeshTelemetryHandle>,
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
            telemetry: None,
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

    pub fn circuits(&self) -> PeerCircuitBreakers {
        self.circuits.clone()
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
        let base_url = match path {
            PeerDirectPath::RealityMesh => peer.mesh_base_url.as_deref().ok_or_else(|| {
                MeshRequestError::InvalidTarget("Mesh is unavailable".to_string())
            })?,
            PeerDirectPath::ApiBaseUrl => &peer.public_base_url,
        };
        let url = join_url(base_url, &request.path_and_query)?;
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
        let (response, verified) = tokio::time::timeout(
            request.total_budget,
            signed_send(
                client,
                &url,
                &request,
                &context,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
            ),
        )
        .await
        .map_err(|_| MeshRequestError::OutcomeUnknown)?
        .map_err(|error| public_transport_error(error, request.allow_ambiguous_fallback))?;
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
                true,
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
                false,
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
        allow_public_fallback: bool,
    ) -> Result<PeerRequestResponse, MeshRequestError> {
        let started = Instant::now();
        let context = RequestContext::now(
            request.route,
            request.cluster_id.clone(),
            request.sender_id.clone(),
            peer.node_id.clone(),
            request.request_id.clone(),
        );
        let mesh_enabled = peer.mesh_base_url.is_some();
        let decision = self
            .circuits
            .before_attempt(&peer.node_id, mesh_enabled)
            .await;
        let mut fallback = matches!(decision, MeshAttemptDecision::SkipOpen);
        let mut mesh_outcome_ambiguous = false;

        if matches!(
            decision,
            MeshAttemptDecision::Attempt | MeshAttemptDecision::Probe
        ) {
            let mesh_url = join_url(
                peer.mesh_base_url.as_deref().expect("checked enabled"),
                &request.path_and_query,
            )?;
            let budget = mesh_attempt_budget(request.total_budget);
            match tokio::time::timeout(
                budget,
                signed_send(
                    &self.mesh,
                    &mesh_url,
                    &request,
                    &context,
                    cluster_ca_key_pem,
                    cluster_ca_cert_pem,
                ),
            )
            .await
            {
                Ok(Ok((response, verified))) => {
                    let transport = mesh_transport_observation(&response);
                    if transport.protocol != MeshTransportProtocol::H2 {
                        self.circuits.release_half_open_probe(&peer.node_id).await;
                        self.record_mesh_protocol_failure(peer).await;
                        self.record_terminal_failure(peer).await;
                        return Err(MeshRequestError::Protocol(
                            "Mesh response did not use HTTP/2".to_string(),
                        ));
                    }
                    if let Some(acknowledgement) =
                        response.headers().get(internal_auth::INTERNAL_ACK_HEADER)
                    {
                        let ack = match acknowledgement.to_str() {
                            Ok(ack) => ack,
                            Err(_) => {
                                self.circuits.release_half_open_probe(&peer.node_id).await;
                                self.record_mesh_protocol_failure(peer).await;
                                self.record_terminal_failure(peer).await;
                                return Err(MeshRequestError::Protocol(
                                    "Mesh response carries a malformed signed acknowledgement"
                                        .to_string(),
                                ));
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
                            self.circuits.release_half_open_probe(&peer.node_id).await;
                            self.record_mesh_protocol_failure(peer).await;
                            self.record_terminal_failure(peer).await;
                            return Err(error.into());
                        }
                        self.record_mesh_success(peer, started, &request, transport)
                            .await;
                        return Ok(PeerRequestResponse::Verified(response));
                    }
                    // An unknown nested admin route bypasses `admin_auth`, so a predecessor's
                    // route miss is intentionally the only accepted acknowledgement-less reply.
                    if allow_unsigned_not_found
                        && response.status() == reqwest::StatusCode::NOT_FOUND
                    {
                        self.record_mesh_success(peer, started, &request, transport)
                            .await;
                        return Ok(PeerRequestResponse::PredecessorNotFound);
                    }
                    self.circuits.release_half_open_probe(&peer.node_id).await;
                    self.record_mesh_protocol_failure(peer).await;
                    self.record_terminal_failure(peer).await;
                    return Err(MeshRequestError::Protocol(
                        "Mesh response did not carry a valid signed acknowledgement".to_string(),
                    ));
                }
                Ok(Err(error)) => {
                    self.record_mesh_transport_failure(
                        peer,
                        MeshPeerReason::TransportError,
                        error.to_string(),
                    )
                    .await;
                    fallback = true;
                    mesh_outcome_ambiguous = true;
                }
                Err(_) => {
                    self.record_mesh_transport_failure(
                        peer,
                        MeshPeerReason::TransportTimeout,
                        "Mesh request timed out".to_string(),
                    )
                    .await;
                    fallback = true;
                    mesh_outcome_ambiguous = true;
                }
            }
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
        let remaining = request.total_budget.saturating_sub(elapsed);
        if remaining.is_zero() {
            self.record_terminal_failure(peer).await;
            return Err(MeshRequestError::OutcomeUnknown);
        }
        let public_url = join_url(&peer.public_base_url, &request.path_and_query)?;
        let response = match self
            .send_public_signed(
                &public_url,
                &request,
                &context,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                remaining,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                self.record_sample(
                    peer,
                    telemetry_sample(
                        TelemetryPath::Public,
                        false,
                        started.elapsed(),
                        fallback,
                        request.updates_active_path,
                        None,
                    ),
                )
                .await;
                return Err(error);
            }
        };
        self.record_sample(
            peer,
            telemetry_sample(
                TelemetryPath::Public,
                true,
                started.elapsed(),
                fallback,
                request.updates_active_path,
                None,
            ),
        )
        .await;
        if fallback && peer.mesh_base_url.is_some() {
            self.record_mesh_reason(peer, MeshPeerReason::FallbackActive)
                .await;
        }
        Ok(PeerRequestResponse::Verified(response))
    }

    async fn record_mesh_success(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        request: &MeshRequest,
        transport: MeshTransportObservation,
    ) {
        let breaker_state = self.circuits.record_success(&peer.node_id).await;
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .set_breaker(&peer.node_id, breaker_state, None)
                .await;
        }
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

    #[allow(clippy::too_many_arguments)]
    async fn send_public_signed(
        &self,
        url: &str,
        request: &MeshRequest,
        context: &RequestContext,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        budget: Duration,
    ) -> Result<reqwest::Response, MeshRequestError> {
        let (response, verified) = tokio::time::timeout(
            budget,
            signed_send(
                &self.public_direct,
                url,
                request,
                context,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
            ),
        )
        .await
        .map_err(|_| MeshRequestError::OutcomeUnknown)?
        .map_err(|error| public_transport_error(error, request.allow_ambiguous_fallback))?;
        let acknowledgement = response
            .headers()
            .get(internal_auth::INTERNAL_ACK_HEADER)
            .ok_or_else(|| {
                MeshRequestError::Protocol(
                    "public response has no signed acknowledgement".to_string(),
                )
            })?;
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

    async fn record_mesh_transport_failure(
        &self,
        peer: &MeshPeerTarget,
        mesh_reason: MeshPeerReason,
        reason: String,
    ) {
        let state = self.circuits.record_retryable_failure(&peer.node_id).await;
        self.record_sample(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                false,
                Duration::ZERO,
                false,
                false,
                None,
            ),
        )
        .await;
        self.record_mesh_reason(peer, mesh_reason).await;
        if let Some(telemetry) = &self.telemetry {
            let message = if state == BreakerState::Open {
                format!("Mesh breaker opened after retryable transport failure: {reason}")
            } else {
                format!("Mesh transport failure: {reason}")
            };
            let _ = telemetry
                .set_breaker(
                    &peer.node_id,
                    state,
                    (state == BreakerState::Open).then_some(message),
                )
                .await;
        }
    }

    async fn record_mesh_protocol_failure(&self, peer: &MeshPeerTarget) {
        self.record_sample(
            peer,
            telemetry_sample(
                TelemetryPath::Mesh,
                false,
                Duration::ZERO,
                false,
                false,
                None,
            ),
        )
        .await;
        self.record_mesh_reason(peer, MeshPeerReason::ProtocolRejected)
            .await;
    }

    async fn record_mesh_reason(&self, peer: &MeshPeerTarget, reason: MeshPeerReason) {
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .set_mesh_reason(&peer.node_id, peer.mesh_base_url.as_deref(), reason)
                .await;
        }
    }

    async fn record_sample(&self, peer: &MeshPeerTarget, sample: MeshTelemetrySample) {
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .record_sample(&peer.node_id, &peer.node_name, sample)
                .await;
        }
        if sample.success && sample.path == TelemetryPath::Mesh {
            self.record_mesh_reason(peer, MeshPeerReason::MeshAvailable)
                .await;
        }
    }

    async fn record_terminal_failure(&self, peer: &MeshPeerTarget) {
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .record_terminal_failure(&peer.node_id, &peer.node_name)
                .await;
        }
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

fn join_url(base: &str, path_and_query: &str) -> Result<String, MeshRequestError> {
    if !path_and_query.starts_with('/') {
        return Err(MeshRequestError::InvalidTarget(
            "request path must start with /".to_string(),
        ));
    }
    let base = reqwest::Url::parse(base)
        .map_err(|error| MeshRequestError::InvalidTarget(error.to_string()))?;
    if !matches!(base.scheme(), "https" | "http") || base.path() != "/" || base.query().is_some() {
        return Err(MeshRequestError::InvalidTarget(
            "peer base URL must be an origin".to_string(),
        ));
    }
    Ok(format!(
        "{}{}",
        base.as_str().trim_end_matches('/'),
        path_and_query
    ))
}

async fn signed_send(
    client: &reqwest::Client,
    url: &str,
    request: &MeshRequest,
    context: &RequestContext,
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
) -> Result<(reqwest::Response, internal_auth::VerifiedRequest), reqwest::Error> {
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
    let mut builder = client
        .request(request.method.clone(), url)
        .body(request.body.clone());
    for (name, value) in &headers {
        builder = builder.header(name, value);
    }
    let response = builder.send().await?;
    Ok((response, verified))
}
#[cfg(test)]
mod peer_target_tests;
