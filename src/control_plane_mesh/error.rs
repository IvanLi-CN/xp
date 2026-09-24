use super::internal_auth;

#[derive(Debug)]
pub enum MeshRequestError {
    InvalidTarget(String),
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
            Self::Auth(value) => Some(value),
            Self::Public(value) => Some(value),
            _ => None,
        }
    }
}
