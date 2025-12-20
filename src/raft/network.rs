use std::fmt::Debug;

use crate::raft::types::{ClientRequest, ClientResponse, NodeId};

/// Raft transport boundary.
///
/// Wave 2 will implement this over HTTPS (likely: axum server + reqwest client) and then adapt it
/// to the OpenRaft `RaftNetwork` / `RaftNetworkFactory` traits.
///
/// Task 003 intentionally does not define the full OpenRaft RPC surface here; instead we keep a
/// project-facing boundary that can stay stable even if we revise HTTP details.
pub trait RaftTransport: Send + Sync + Debug + 'static {
    fn name(&self) -> &'static str;

    /// Send a client write request to the leader (or to a target node for forwarding).
    ///
    /// This is a higher-level boundary than raw Raft RPC; it matches the project need of
    /// "follower forwards writes to leader".
    fn forward_client_write(
        &self,
        _target: NodeId,
        _req: ClientRequest,
    ) -> impl std::future::Future<Output = anyhow::Result<ClientResponse>> + Send;
}

