use crate::control_plane_mesh::MeshRequestError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaftWriteDispatchState {
    NotDispatched,
    DispatchedNoVerifiedResponse,
    Unknown,
}

impl RaftWriteDispatchState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotDispatched => "not_dispatched",
            Self::DispatchedNoVerifiedResponse => "dispatched_no_verified_response",
            Self::Unknown => "unknown",
        }
    }

    fn confidence(self) -> &'static str {
        match self {
            Self::NotDispatched => "confirmed",
            Self::DispatchedNoVerifiedResponse | Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug)]
pub struct RaftWriteError {
    dispatch_state: RaftWriteDispatchState,
    cause: &'static str,
    target_node_id: Option<String>,
    support_id: String,
    remote_status: Option<u16>,
}

impl RaftWriteError {
    pub fn not_dispatched(
        cause: &'static str,
        target_node_id: Option<String>,
        support_id: impl Into<String>,
    ) -> Self {
        Self {
            dispatch_state: RaftWriteDispatchState::NotDispatched,
            cause,
            target_node_id,
            support_id: support_id.into(),
            remote_status: None,
        }
    }

    pub fn outcome_unknown(
        cause: &'static str,
        dispatch_state: RaftWriteDispatchState,
        target_node_id: Option<String>,
        support_id: impl Into<String>,
    ) -> Self {
        debug_assert!(dispatch_state != RaftWriteDispatchState::NotDispatched);
        Self {
            dispatch_state,
            cause,
            target_node_id,
            support_id: support_id.into(),
            remote_status: None,
        }
    }

    pub fn with_remote_status(mut self, status: u16) -> Self {
        self.remote_status = Some(status);
        self
    }

    pub fn dispatch_state(&self) -> RaftWriteDispatchState {
        self.dispatch_state
    }

    pub fn cause(&self) -> &'static str {
        self.cause
    }

    pub fn target_node_id(&self) -> Option<&str> {
        self.target_node_id.as_deref()
    }

    pub fn support_id(&self) -> &str {
        &self.support_id
    }

    pub fn remote_status(&self) -> Option<u16> {
        self.remote_status
    }

    pub fn confidence(&self) -> &'static str {
        self.dispatch_state.confidence()
    }
}

impl std::fmt::Display for RaftWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.dispatch_state {
            RaftWriteDispatchState::NotDispatched => f.write_str("Raft write was not dispatched"),
            RaftWriteDispatchState::DispatchedNoVerifiedResponse
            | RaftWriteDispatchState::Unknown => f.write_str("Raft write outcome is unknown"),
        }
    }
}

impl std::error::Error for RaftWriteError {}

pub(crate) fn dispatch_state(error: &MeshRequestError) -> RaftWriteDispatchState {
    match error {
        MeshRequestError::InvalidTarget(_)
        | MeshRequestError::PreDispatchAuth(_)
        | MeshRequestError::PreDispatchTimeout
        | MeshRequestError::CircuitOpen {
            dispatched: false, ..
        } => RaftWriteDispatchState::NotDispatched,
        MeshRequestError::OutcomeUnknown
        | MeshRequestError::Reverse(_)
        | MeshRequestError::CircuitOpen {
            dispatched: true, ..
        } => RaftWriteDispatchState::Unknown,
        MeshRequestError::Auth(_)
        | MeshRequestError::TransportTimeout
        | MeshRequestError::Protocol(_)
        | MeshRequestError::ReverseTimeout
        | MeshRequestError::Public(_) => RaftWriteDispatchState::DispatchedNoVerifiedResponse,
    }
}

fn error_cause(error: &MeshRequestError) -> &'static str {
    match error {
        MeshRequestError::InvalidTarget(_) => "invalid_target",
        MeshRequestError::PreDispatchAuth(_) => "pre_dispatch_authentication",
        MeshRequestError::PreDispatchTimeout => "pre_dispatch_timeout",
        MeshRequestError::Auth(_) => "peer_authentication",
        MeshRequestError::OutcomeUnknown => "outcome_unknown",
        MeshRequestError::TransportTimeout => "transport_timeout",
        MeshRequestError::Protocol(_) => "protocol_error",
        MeshRequestError::Reverse(_) => "route_unavailable",
        MeshRequestError::ReverseTimeout => "reverse_timeout",
        MeshRequestError::Public(_) => "public_transport",
        MeshRequestError::CircuitOpen { .. } => "circuit_open",
    }
}

pub(crate) fn from_mesh(
    error: MeshRequestError,
    target_node_id: String,
    support_id: &str,
) -> RaftWriteError {
    let state = dispatch_state(&error);
    let cause = error_cause(&error);
    if state == RaftWriteDispatchState::NotDispatched {
        RaftWriteError::not_dispatched(cause, Some(target_node_id), support_id)
    } else {
        RaftWriteError::outcome_unknown(cause, state, Some(target_node_id), support_id)
    }
}
