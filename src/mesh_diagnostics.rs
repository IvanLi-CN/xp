use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeshRouteKind {
    DirectMesh,
    ReverseRelay,
    Public,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeshFailureClass {
    CircuitOpen,
    PreResponseTimeout,
    PreResponseTransport,
    UnsignedResponse,
    AcknowledgementMissing,
    AcknowledgementInvalid,
    OutcomeUnknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeshAcknowledgementState {
    NotObserved,
    Missing,
    Invalid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeshDispatchState {
    NotDispatched,
    DispatchedNoVerifiedResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeshRouteAttempt {
    pub route: MeshRouteKind,
    pub failure: MeshFailureClass,
    pub acknowledgement: MeshAcknowledgementState,
    pub dispatch: MeshDispatchState,
    pub observed_at: String,
    pub request_id: String,
    pub elapsed_ms: u32,
    pub retry_count: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeshPublicFailure {
    pub observed_at: String,
    pub request_id: String,
    pub failure: MeshFailureClass,
    pub acknowledgement: MeshAcknowledgementState,
    pub dispatch: MeshDispatchState,
    pub elapsed_ms: u32,
    pub retry_count: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
}

pub(crate) fn public_failure_is_newer(
    candidate: &MeshPublicFailure,
    previous: &MeshPublicFailure,
) -> bool {
    match (
        chrono::DateTime::parse_from_rfc3339(&candidate.observed_at),
        chrono::DateTime::parse_from_rfc3339(&previous.observed_at),
    ) {
        (Ok(candidate_at), Ok(previous_at)) => {
            candidate_at > previous_at
                || (candidate_at == previous_at && candidate.request_id >= previous.request_id)
        }
        (Ok(_), Err(_)) => true,
        (Err(_), Ok(_)) => false,
        (Err(_), Err(_)) => candidate.request_id >= previous.request_id,
    }
}
