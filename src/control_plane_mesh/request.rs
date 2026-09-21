use super::internal_auth::InternalRoute;
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
