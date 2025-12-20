use std::io::Cursor;

use serde::{Deserialize, Serialize};

/// Raft node identifier type for this project.
pub type NodeId = u64;

/// Raft node metadata stored in membership config and exposed to networking.
///
/// Milestone 5 also needs additional *local* metadata (certs, keys, cluster id, etc.) that does
/// not belong in the Raft state machine; those will live outside this struct.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeMeta {
    /// A human-friendly node name (optional).
    pub name: String,

    /// The admin/API base URL (used by clients and for follower->leader forwarding in Wave 2).
    pub api_base_url: String,

    /// The Raft RPC endpoint identifier.
    ///
    /// Wave 2 will define whether this is the same as `api_base_url` or a dedicated internal
    /// endpoint, depending on the chosen HTTPS wiring.
    pub raft_endpoint: String,
}

/// State-machine command (client request) submitted to Raft.
///
/// Task 003 keeps this intentionally minimal; Milestone 5 will evolve it into "desired state"
/// updates (Nodes/Endpoints/Users/Grants).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientRequest {
    /// A no-op command used by tests and smoke wiring.
    Noop,
}

/// State-machine response to a committed `ClientRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientResponse {
    /// Acknowledge `ClientRequest::Noop`.
    Ok,
}

/// OpenRaft type configuration for this project.
///
/// OpenRaft's storage v2 model separates `RaftLogStorage` and `RaftStateMachine`, which matches
/// our desired "WAL + snapshot + desired-state state machine" architecture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeConfig;

impl openraft::RaftTypeConfig for TypeConfig {
    type D = ClientRequest;
    type R = ClientResponse;

    type NodeId = NodeId;
    type Node = NodeMeta;

    type Entry = openraft::impls::Entry<TypeConfig>;
    type Responder = openraft::impls::OneshotResponder<TypeConfig>;
    type AsyncRuntime = openraft::impls::TokioRuntime;

    // Requires tokio `io-util` feature for AsyncRead/Write/Seek impls on Cursor.
    type SnapshotData = Cursor<Vec<u8>>;
}

