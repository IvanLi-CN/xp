use super::internal_auth::InternalRoute;
use super::*;
use std::time::Duration;

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

pub(crate) enum CapabilityProbeResponse {
    Verified(reqwest::Response),
    PredecessorNotFound,
}

pub(super) enum PeerRequestResponse {
    Verified(reqwest::Response),
    PredecessorNotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerDirectPath {
    RealityMesh,
    ApiBaseUrl,
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
        self.send_peer_request_with_body_deadline(
            peer,
            request,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            None,
        )
        .await
    }

    /// Sends through Mesh first, then public, with a dedicated response-body lease. The request
    /// budget still bounds admission and first-byte delivery; the lease begins once a verified
    /// response is ready for body consumption.
    pub(crate) async fn send_peer_request_with_body_lease(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        body_lease: Duration,
    ) -> Result<reqwest::Response, MeshRequestError> {
        self.send_peer_request_with_body_deadline(
            peer,
            request,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            Some(body_lease),
        )
        .await
    }

    async fn send_peer_request_with_body_deadline(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        body_lease: Option<Duration>,
    ) -> Result<reqwest::Response, MeshRequestError> {
        match self
            .send_peer_request_with_legacy_not_found(
                peer,
                request,
                cluster_ca_key_pem,
                cluster_ca_cert_pem,
                false,
                gate::PublicFallbackPolicy::Always,
                body_lease,
            )
            .await?
        {
            PeerRequestResponse::Verified(response) => Ok(response),
            PeerRequestResponse::PredecessorNotFound => Err(MeshRequestError::Protocol(
                "unexpected predecessor capability response".to_string(),
            )),
        }
    }
}
