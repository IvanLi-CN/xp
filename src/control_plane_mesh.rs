use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{
    internal_auth::{self, InternalRoute, RequestContext},
    mesh_telemetry::{BreakerState, MeshTelemetryHandle, TelemetryPath},
};

pub const MESH_FAILURES_BEFORE_OPEN: u8 = 3;
pub const MESH_BACKOFF: [Duration; 5] = [
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(240),
    Duration::from_secs(300),
];

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

    /// Returns the externally visible state. Only retryable transport errors are passed here;
    /// authenticated HTTP responses and auth/protocol failures must never trip this breaker.
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
    pub public_base_url: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshProxyStatus {
    Disabled,
    Ready,
    Fallback,
    Degraded,
}

impl MeshProxyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Ready => "ready",
            Self::Fallback => "fallback",
            Self::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshProxySnapshot {
    pub status: MeshProxyStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fallback_at: Option<String>,
}

#[derive(Clone)]
pub struct MeshProxyStateHandle {
    inner: Arc<Mutex<MeshProxySnapshot>>,
}

impl MeshProxyStateHandle {
    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MeshProxySnapshot {
                status: MeshProxyStatus::Disabled,
                fallback_reason: None,
                last_fallback_at: None,
            })),
        }
    }

    pub fn ready() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MeshProxySnapshot {
                status: MeshProxyStatus::Ready,
                fallback_reason: None,
                last_fallback_at: None,
            })),
        }
    }

    pub async fn snapshot(&self) -> MeshProxySnapshot {
        self.inner.lock().await.clone()
    }

    pub async fn mark_ready(&self) {
        let mut inner = self.inner.lock().await;
        inner.status = MeshProxyStatus::Ready;
        inner.fallback_reason = None;
        inner.last_fallback_at = None;
    }

    pub async fn mark_fallback(&self, reason: impl Into<String>) {
        let mut inner = self.inner.lock().await;
        inner.status = MeshProxyStatus::Fallback;
        inner.fallback_reason = Some(reason.into());
        inner.last_fallback_at = Some(Utc::now().to_rfc3339());
    }

    pub async fn mark_degraded(&self, reason: impl Into<String>) {
        let mut inner = self.inner.lock().await;
        inner.status = MeshProxyStatus::Degraded;
        inner.fallback_reason = Some(reason.into());
        inner.last_fallback_at = Some(Utc::now().to_rfc3339());
    }
}

#[derive(Debug)]
pub enum MeshProxyError {
    InvalidProxyUrl { proxy_url: String, message: String },
}

impl std::fmt::Display for MeshProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProxyUrl { proxy_url, message } => {
                write!(f, "invalid proxy url {proxy_url}: {message}")
            }
        }
    }
}

impl std::error::Error for MeshProxyError {}

pub fn apply_optional_proxy(
    builder: reqwest::ClientBuilder,
    proxy_url: Option<&str>,
) -> Result<reqwest::ClientBuilder, MeshProxyError> {
    let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(builder);
    };

    let proxy = reqwest::Proxy::all(proxy_url).map_err(|err| MeshProxyError::InvalidProxyUrl {
        proxy_url: proxy_url.to_string(),
        message: err.to_string(),
    })?;
    Ok(builder.proxy(proxy))
}

#[derive(Clone)]
pub struct MeshAwareHttpClient {
    direct: reqwest::Client,
    relay: Option<reqwest::Client>,
    state: MeshProxyStateHandle,
    circuits: PeerCircuitBreakers,
    telemetry: Option<MeshTelemetryHandle>,
}

impl MeshAwareHttpClient {
    pub fn new(
        direct: reqwest::Client,
        relay: Option<reqwest::Client>,
        state: MeshProxyStateHandle,
    ) -> Self {
        Self {
            direct,
            relay,
            state,
            circuits: PeerCircuitBreakers::default(),
            telemetry: None,
        }
    }

    pub fn direct(&self) -> &reqwest::Client {
        &self.direct
    }

    pub fn relay_enabled(&self) -> bool {
        self.relay.is_some()
    }

    pub fn state(&self) -> MeshProxyStateHandle {
        self.state.clone()
    }

    pub fn with_mesh_observability(mut self, telemetry: MeshTelemetryHandle) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    pub fn circuits(&self) -> PeerCircuitBreakers {
        self.circuits.clone()
    }

    pub async fn send_with_fallback<F>(
        &self,
        budget: Duration,
        build_request: F,
    ) -> Result<reqwest::Response, reqwest::Error>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        if let Some(relay) = &self.relay {
            let relay_budget =
                std::cmp::min(budget / 2, Duration::from_secs(1)).max(Duration::from_millis(1));
            match tokio::time::timeout(relay_budget, build_request(relay).send()).await {
                Ok(Ok(response)) => {
                    self.state.mark_ready().await;
                    return Ok(response);
                }
                Err(_) => {
                    let relay_reason = format!("relay timed out after {:?}", relay_budget);
                    tracing::warn!(
                        target = "xp::control_plane_mesh",
                        error = %relay_reason,
                        "control-plane relay request failed; falling back to direct"
                    );

                    match build_request(&self.direct).send().await {
                        Ok(response) => {
                            self.state.mark_fallback(relay_reason).await;
                            return Ok(response);
                        }
                        Err(direct_err) => {
                            self.state
                                .mark_degraded(format!(
                                    "relay timed out; direct failed: {direct_err}"
                                ))
                                .await;
                            return Err(direct_err);
                        }
                    }
                }
                Ok(Err(relay_err)) => {
                    let relay_reason = relay_err.to_string();
                    tracing::warn!(
                        target = "xp::control_plane_mesh",
                        error = %relay_reason,
                        "control-plane relay request failed; falling back to direct"
                    );

                    match build_request(&self.direct).send().await {
                        Ok(response) => {
                            self.state.mark_fallback(relay_reason).await;
                            return Ok(response);
                        }
                        Err(direct_err) => {
                            self.state
                                .mark_degraded(format!(
                                    "relay failed: {relay_reason}; direct failed: {direct_err}"
                                ))
                                .await;
                            return Err(direct_err);
                        }
                    }
                }
            }
        }

        build_request(&self.direct).send().await
    }

    /// Sends a signed control-plane request through the peer's REALITY fallback first, then the
    /// public origin if and only if the Mesh failure was an unambiguous retryable transport
    /// failure. Any valid acknowledgement (including 4xx/5xx) is authoritative.
    pub async fn send_peer_request(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<reqwest::Response, MeshRequestError> {
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

        // A half-open circuit gets exactly one live request. A valid signed acknowledgement
        // immediately closes the circuit; a transport failure reopens it with the next delay.
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
                    &self.direct,
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
                    if let Some(ack) = response
                        .headers()
                        .get(internal_auth::INTERNAL_ACK_HEADER)
                        .and_then(|value| value.to_str().ok())
                    {
                        internal_auth::verify_ack_v2(
                            cluster_ca_key_pem,
                            cluster_ca_cert_pem,
                            &verified,
                            &peer.node_id,
                            response.status().as_u16(),
                            ack,
                        )?;
                        self.record_sample(
                            peer,
                            TelemetryPath::Mesh,
                            true,
                            started.elapsed(),
                            false,
                        )
                        .await;
                        self.circuits.record_success(&peer.node_id).await;
                        return Ok(response);
                    }
                    return Err(MeshRequestError::Protocol(
                        "Mesh response did not carry a valid signed acknowledgement".to_string(),
                    ));
                }
                Ok(Err(error)) => {
                    self.record_mesh_transport_failure(peer, error.to_string())
                        .await;
                    fallback = true;
                }
                Err(_) => {
                    self.record_mesh_transport_failure(peer, "Mesh request timed out".to_string())
                        .await;
                    fallback = true;
                }
            }
        }

        if !request.allow_ambiguous_fallback && fallback {
            return Err(MeshRequestError::OutcomeUnknown);
        }
        let elapsed = started.elapsed();
        let remaining = request.total_budget.saturating_sub(elapsed);
        if remaining.is_zero() {
            return Err(MeshRequestError::OutcomeUnknown);
        }
        let public_url = join_url(&peer.public_base_url, &request.path_and_query)?;
        let response = self
            .send_public_signed(
                &public_url,
                &request,
                &context,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                remaining,
            )
            .await?;
        self.record_sample(
            peer,
            TelemetryPath::Public,
            true,
            started.elapsed(),
            fallback,
        )
        .await;
        if fallback {
            self.state
                .mark_fallback("per-peer Reality fallback unavailable")
                .await;
        }
        Ok(response)
    }

    async fn send_public_signed(
        &self,
        url: &str,
        request: &MeshRequest,
        context: &RequestContext,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        budget: Duration,
    ) -> Result<reqwest::Response, MeshRequestError> {
        let (response, verified) = if let Some(relay) = &self.relay {
            let relay_budget = std::cmp::max(Duration::from_millis(1), budget / 2);
            let relay_started = Instant::now();
            match tokio::time::timeout(
                relay_budget,
                signed_send(
                    relay,
                    url,
                    request,
                    context,
                    cluster_ca_key_pem,
                    cluster_ca_cert_pem,
                ),
            )
            .await
            {
                Ok(Ok(value)) => value,
                Ok(Err(_)) | Err(_) => tokio::time::timeout(
                    budget.saturating_sub(relay_started.elapsed()),
                    signed_send(
                        &self.direct,
                        url,
                        request,
                        context,
                        cluster_ca_key_pem,
                        cluster_ca_cert_pem,
                    ),
                )
                .await
                .map_err(|_| MeshRequestError::OutcomeUnknown)?
                .map_err(MeshRequestError::Public)?,
            }
        } else {
            tokio::time::timeout(
                budget,
                signed_send(
                    &self.direct,
                    url,
                    request,
                    context,
                    cluster_ca_key_pem,
                    cluster_ca_cert_pem,
                ),
            )
            .await
            .map_err(|_| MeshRequestError::OutcomeUnknown)?
            .map_err(MeshRequestError::Public)?
        };
        let ack = response
            .headers()
            .get(internal_auth::INTERNAL_ACK_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                MeshRequestError::Protocol(
                    "public response has no signed acknowledgement".to_string(),
                )
            })?;
        internal_auth::verify_ack_v2(
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            &verified,
            &context.target_id,
            response.status().as_u16(),
            ack,
        )?;
        Ok(response)
    }

    async fn record_mesh_transport_failure(&self, peer: &MeshPeerTarget, reason: String) {
        let state = self.circuits.record_retryable_failure(&peer.node_id).await;
        self.record_sample(peer, TelemetryPath::Mesh, false, Duration::ZERO, false)
            .await;
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

    async fn record_sample(
        &self,
        peer: &MeshPeerTarget,
        path: TelemetryPath,
        success: bool,
        elapsed: Duration,
        fallback: bool,
    ) {
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .record_sample(
                    &peer.node_id,
                    &peer.node_name,
                    path,
                    success,
                    success.then_some(elapsed.as_millis().min(u32::MAX as u128) as u32),
                    fallback,
                )
                .await;
        }
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
    // Signing failures reflect malformed local inputs. They cannot be represented as a network
    // error, so use a build-time request error by asserting the internal contract here.
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
mod tests {
    use super::*;

    #[test]
    fn mesh_proxy_status_strings_are_stable() {
        assert_eq!(MeshProxyStatus::Disabled.as_str(), "disabled");
        assert_eq!(MeshProxyStatus::Ready.as_str(), "ready");
        assert_eq!(MeshProxyStatus::Fallback.as_str(), "fallback");
        assert_eq!(MeshProxyStatus::Degraded.as_str(), "degraded");
    }

    #[test]
    fn invalid_proxy_url_is_rejected() {
        let builder = reqwest::Client::builder();
        let err = apply_optional_proxy(builder, Some("not a url")).unwrap_err();
        assert!(err.to_string().contains("invalid proxy url"));
    }

    #[test]
    fn mesh_budget_reserves_the_public_remainder() {
        assert_eq!(
            mesh_attempt_budget(Duration::from_millis(100)),
            Duration::from_millis(500)
        );
        assert_eq!(
            mesh_attempt_budget(Duration::from_secs(6)),
            Duration::from_secs(2)
        );
        assert_eq!(
            mesh_attempt_budget(Duration::from_secs(30)),
            Duration::from_secs(5)
        );
    }

    #[tokio::test]
    async fn peer_breaker_opens_after_three_transport_failures() {
        let breakers = PeerCircuitBreakers::default();
        assert_eq!(
            breakers.before_attempt("peer-a", true).await,
            MeshAttemptDecision::Attempt
        );
        assert_eq!(
            breakers.record_retryable_failure("peer-a").await,
            BreakerState::Closed
        );
        assert_eq!(
            breakers.record_retryable_failure("peer-a").await,
            BreakerState::Closed
        );
        assert_eq!(
            breakers.record_retryable_failure("peer-a").await,
            BreakerState::Open
        );
        assert_eq!(
            breakers.before_attempt("peer-a", true).await,
            MeshAttemptDecision::SkipOpen
        );
        assert_eq!(
            breakers.record_success("peer-a").await,
            BreakerState::Closed
        );
    }

    #[tokio::test]
    async fn half_open_allows_exactly_one_mesh_probe() {
        let breakers = PeerCircuitBreakers::default();
        breakers.peers.lock().await.insert(
            "peer-a".to_string(),
            PeerCircuit {
                failures: MESH_FAILURES_BEFORE_OPEN,
                open_count: 1,
                retry_at: Some(Instant::now() - Duration::from_secs(1)),
                half_open_in_flight: false,
            },
        );

        assert_eq!(
            breakers.before_attempt("peer-a", true).await,
            MeshAttemptDecision::Probe
        );
        assert_eq!(
            breakers.before_attempt("peer-a", true).await,
            MeshAttemptDecision::SkipOpen
        );
        assert_eq!(
            breakers.record_success("peer-a").await,
            BreakerState::Closed
        );
    }
}
