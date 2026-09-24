use super::internal_auth;

#[derive(Debug)]
pub enum MeshRequestError {
    InvalidTarget(String),
    Auth(internal_auth::AuthError),
    OutcomeUnknown,
    Protocol(String),
    UnsignedResponse {
        status: u16,
    },
    AcknowledgementMissing {
        status: u16,
    },
    AcknowledgementInvalid,
    Reverse(String),
    Public(reqwest::Error),
    PublicTransport {
        error: reqwest::Error,
        retry_count: u8,
    },
    PublicTimeout {
        retry_count: u8,
    },
    CircuitOpen {
        path: &'static str,
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
            Self::Protocol(value) => write!(f, "Mesh protocol error: {value}"),
            Self::UnsignedResponse { .. } => f.write_str("Mesh response is unsigned"),
            Self::AcknowledgementMissing { .. } => {
                f.write_str("Mesh response has no signed acknowledgement")
            }
            Self::AcknowledgementInvalid => {
                f.write_str("Mesh response has an invalid signed acknowledgement")
            }
            Self::Reverse(value) => write!(f, "reverse relay failed: {value}"),
            Self::Public(value) => write!(f, "public fallback failed: {value}"),
            Self::PublicTransport { error, .. } => {
                write!(f, "public fallback transport failed: {error}")
            }
            Self::PublicTimeout { .. } => f.write_str("public fallback timed out before response"),
            Self::CircuitOpen { path } => write!(f, "{path} circuit is cooling down"),
        }
    }
}

impl std::error::Error for MeshRequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Auth(value) => Some(value),
            Self::Public(value) => Some(value),
            Self::PublicTransport { error, .. } => Some(error),
            _ => None,
        }
    }
}
