use std::fmt::Debug;

use crate::raft::types::{ClientRequest, ClientResponse};

/// Desired-state state machine boundary.
///
/// Milestone 5 requirement: state machine stores only the desired state (Nodes/Endpoints/Users/Grants).
/// High-frequency runtime data (e.g. usage counters) must stay out of Raft.
pub trait StateMachineStore: Send + Sync + Debug + 'static {
    fn apply(
        &self,
        _req: ClientRequest,
    ) -> impl std::future::Future<Output = anyhow::Result<ClientResponse>> + Send;
}
