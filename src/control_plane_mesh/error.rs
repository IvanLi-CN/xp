use super::circuit::MeshAttemptDecision;
use super::internal_auth;

#[derive(Debug)]
pub enum MeshRequestError {
    InvalidTarget(String),
    PreDispatchAuth(internal_auth::AuthError),
    PreDispatchTimeout,
    Auth(internal_auth::AuthError),
    OutcomeUnknown,
    TransportTimeout,
    Protocol(String),
    Reverse(String),
    ReverseTimeout,
    Public(reqwest::Error),
    CircuitOpen {
        path: &'static str,
        dispatched: bool,
    },
}

pub(super) fn classify_mesh_failure(
    admission_timed_out: bool,
    mesh_outcome_ambiguous: bool,
    mesh_outcome_timed_out: bool,
    decision: MeshAttemptDecision,
) -> MeshRequestError {
    if admission_timed_out && !mesh_outcome_ambiguous {
        MeshRequestError::PreDispatchTimeout
    } else if matches!(decision, MeshAttemptDecision::Disabled) {
        MeshRequestError::InvalidTarget("Mesh is unavailable".to_string())
    } else if mesh_outcome_timed_out {
        MeshRequestError::TransportTimeout
    } else {
        MeshRequestError::OutcomeUnknown
    }
}

pub(super) fn public_timeout(
    mesh_outcome_ambiguous: bool,
    mesh_outcome_timed_out: bool,
    decision: MeshAttemptDecision,
) -> MeshRequestError {
    classify_mesh_failure(
        !mesh_outcome_ambiguous,
        mesh_outcome_ambiguous,
        mesh_outcome_timed_out,
        decision,
    )
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
            Self::PreDispatchAuth(value) => {
                write!(
                    f,
                    "local internal authentication failed before dispatch: {value}"
                )
            }
            Self::PreDispatchTimeout => f.write_str("request budget expired before dispatch"),
            Self::Auth(value) => write!(f, "internal authentication error: {value}"),
            Self::OutcomeUnknown => {
                f.write_str("Mesh request outcome is unknown; it may already have been applied")
            }
            Self::TransportTimeout => {
                f.write_str("Mesh request timed out before a verified response")
            }
            Self::Protocol(value) => write!(f, "Mesh protocol error: {value}"),
            Self::Reverse(value) => write!(f, "reverse relay failed: {value}"),
            Self::ReverseTimeout => f.write_str("reverse relay timed out before response headers"),
            Self::Public(value) => write!(f, "public fallback failed: {value}"),
            Self::CircuitOpen { path, .. } => write!(f, "{path} circuit is cooling down"),
        }
    }
}

impl std::error::Error for MeshRequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PreDispatchAuth(value) => Some(value),
            Self::Auth(value) => Some(value),
            Self::Public(value) => Some(value),
            _ => None,
        }
    }
}
