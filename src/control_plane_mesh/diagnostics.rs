use std::time::Instant;

use crate::mesh_telemetry::{
    BreakerState, MeshAcknowledgementState, MeshDispatchState, MeshFailureClass, MeshPublicFailure,
    MeshRouteAttempt, MeshRouteKind,
};

use super::{
    CapabilityProbeResponse, LEGACY_CAPABILITIES_PROBE_PATH, MeshAwareHttpClient, MeshPeerTarget,
    MeshRequest, MeshRequestError, PeerRequestResponse, gate,
};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub(crate) struct MeshRequestDiagnostics {
    pub request_id: String,
    pub route_attempts: Vec<MeshRouteAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_circuit: Option<BreakerState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_public_failure: Option<MeshPublicFailure>,
}

#[derive(Debug)]
pub(crate) struct MeshRequestFailure {
    pub diagnostics: MeshRequestDiagnostics,
}

pub(super) struct MeshRequestOptions<'a> {
    pub allow_unsigned_not_found: bool,
    pub public_fallback_policy: gate::PublicFallbackPolicy,
    pub diagnostics: &'a mut MeshRequestDiagnostics,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MeshAttemptFailure {
    pub failure: MeshFailureClass,
    pub acknowledgement: MeshAcknowledgementState,
    pub dispatch: MeshDispatchState,
    pub retry_count: u8,
    pub http_status: Option<u16>,
}

impl MeshRequestDiagnostics {
    pub(super) fn new(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            ..Self::default()
        }
    }

    pub(super) fn record(
        &mut self,
        route: MeshRouteKind,
        request_id: &str,
        started: Instant,
        failure: MeshAttemptFailure,
    ) {
        if self.route_attempts.len() >= 8 {
            return;
        }
        self.route_attempts.push(MeshRouteAttempt {
            route,
            failure: failure.failure,
            acknowledgement: failure.acknowledgement,
            dispatch: failure.dispatch,
            observed_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            request_id: request_id.to_string(),
            elapsed_ms: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
            retry_count: failure.retry_count,
            http_status: failure.http_status,
        });
    }

    pub(super) fn latest_public_failure(&self) -> Option<MeshPublicFailure> {
        self.route_attempts
            .iter()
            .rev()
            .find(|attempt| attempt.route == MeshRouteKind::Public)
            .map(|attempt| MeshPublicFailure {
                observed_at: attempt.observed_at.clone(),
                request_id: attempt.request_id.clone(),
                failure: attempt.failure,
                acknowledgement: attempt.acknowledgement,
                dispatch: attempt.dispatch,
                elapsed_ms: attempt.elapsed_ms,
                retry_count: attempt.retry_count,
                http_status: attempt.http_status,
            })
    }
}

pub(crate) fn failure_for_error(error: &MeshRequestError) -> MeshAttemptFailure {
    match error {
        MeshRequestError::CircuitOpen { .. } => MeshAttemptFailure {
            failure: MeshFailureClass::CircuitOpen,
            acknowledgement: MeshAcknowledgementState::NotObserved,
            dispatch: MeshDispatchState::NotDispatched,
            retry_count: 0,
            http_status: None,
        },
        MeshRequestError::AcknowledgementMissing { status } => MeshAttemptFailure {
            failure: MeshFailureClass::AcknowledgementMissing,
            acknowledgement: MeshAcknowledgementState::Missing,
            dispatch: MeshDispatchState::DispatchedNoVerifiedResponse,
            retry_count: 0,
            http_status: Some(*status),
        },
        MeshRequestError::UnsignedResponse { status } => MeshAttemptFailure {
            failure: MeshFailureClass::UnsignedResponse,
            acknowledgement: MeshAcknowledgementState::Missing,
            dispatch: MeshDispatchState::DispatchedNoVerifiedResponse,
            retry_count: 0,
            http_status: Some(*status),
        },
        MeshRequestError::AcknowledgementInvalid => MeshAttemptFailure {
            failure: MeshFailureClass::AcknowledgementInvalid,
            acknowledgement: MeshAcknowledgementState::Invalid,
            dispatch: MeshDispatchState::DispatchedNoVerifiedResponse,
            retry_count: 0,
            http_status: None,
        },
        MeshRequestError::Public(error) => MeshAttemptFailure {
            failure: if error.is_timeout() {
                MeshFailureClass::PreResponseTimeout
            } else {
                MeshFailureClass::PreResponseTransport
            },
            acknowledgement: MeshAcknowledgementState::NotObserved,
            dispatch: MeshDispatchState::DispatchedNoVerifiedResponse,
            retry_count: 0,
            http_status: None,
        },
        MeshRequestError::PublicTransport { error, retry_count } => MeshAttemptFailure {
            failure: if error.is_timeout() {
                MeshFailureClass::PreResponseTimeout
            } else {
                MeshFailureClass::PreResponseTransport
            },
            acknowledgement: MeshAcknowledgementState::NotObserved,
            dispatch: MeshDispatchState::DispatchedNoVerifiedResponse,
            retry_count: *retry_count,
            http_status: None,
        },
        MeshRequestError::PublicTimeout { retry_count } => MeshAttemptFailure {
            failure: MeshFailureClass::PreResponseTimeout,
            acknowledgement: MeshAcknowledgementState::NotObserved,
            dispatch: MeshDispatchState::DispatchedNoVerifiedResponse,
            retry_count: *retry_count,
            http_status: None,
        },
        MeshRequestError::OutcomeUnknown => MeshAttemptFailure {
            failure: MeshFailureClass::OutcomeUnknown,
            acknowledgement: MeshAcknowledgementState::NotObserved,
            dispatch: MeshDispatchState::DispatchedNoVerifiedResponse,
            retry_count: 0,
            http_status: None,
        },
        _ => MeshAttemptFailure {
            failure: MeshFailureClass::PreResponseTransport,
            acknowledgement: MeshAcknowledgementState::NotObserved,
            dispatch: MeshDispatchState::NotDispatched,
            retry_count: 0,
            http_status: None,
        },
    }
}

impl MeshAwareHttpClient {
    /// Sends through Mesh first, then public only after a retryable transport failure.
    pub async fn send_peer_request(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<reqwest::Response, MeshRequestError> {
        let mut diagnostics = MeshRequestDiagnostics::default();
        match self
            .send_peer_request_with_legacy_not_found(
                peer,
                request,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                MeshRequestOptions {
                    allow_unsigned_not_found: false,
                    public_fallback_policy: gate::PublicFallbackPolicy::Always,
                    diagnostics: &mut diagnostics,
                },
            )
            .await?
        {
            PeerRequestResponse::Verified(response) => Ok(response),
            PeerRequestResponse::PredecessorNotFound => Err(MeshRequestError::Protocol(
                "unexpected predecessor capability response".to_string(),
            )),
        }
    }

    pub(crate) async fn send_peer_request_with_diagnostics(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<(reqwest::Response, MeshRequestDiagnostics), MeshRequestFailure> {
        let mut diagnostics = MeshRequestDiagnostics::new(request.request_id.clone());
        diagnostics.public_circuit = Some(self.circuits.public_state(&peer.node_id).await);
        let response = match self
            .send_peer_request_with_legacy_not_found(
                peer,
                request,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                MeshRequestOptions {
                    allow_unsigned_not_found: false,
                    public_fallback_policy: gate::PublicFallbackPolicy::Always,
                    diagnostics: &mut diagnostics,
                },
            )
            .await
        {
            Ok(response) => response,
            Err(_error) => {
                diagnostics.public_circuit = Some(self.circuits.public_state(&peer.node_id).await);
                if let Some(telemetry) = &self.telemetry {
                    diagnostics.last_public_failure = telemetry
                        .last_public_failure(&peer.node_id)
                        .await
                        .filter(|failure| failure.request_id == diagnostics.request_id);
                }
                return Err(MeshRequestFailure { diagnostics });
            }
        };
        match response {
            PeerRequestResponse::Verified(response) => Ok((response, diagnostics)),
            PeerRequestResponse::PredecessorNotFound => Err(MeshRequestFailure { diagnostics }),
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
            || request.route != crate::internal_auth::InternalRoute::MeshV2
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
                MeshRequestOptions {
                    allow_unsigned_not_found: true,
                    public_fallback_policy: gate::PublicFallbackPolicy::WhenMeshDisabled,
                    diagnostics: &mut MeshRequestDiagnostics::default(),
                },
            )
            .await?;
        Ok(match response {
            PeerRequestResponse::Verified(response) => CapabilityProbeResponse::Verified(response),
            PeerRequestResponse::PredecessorNotFound => {
                CapabilityProbeResponse::PredecessorNotFound
            }
        })
    }
}
