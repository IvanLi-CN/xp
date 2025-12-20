//! Raft module skeleton for Milestone 5 (WAL + snapshot + desired-state state machine).
//!
//! This module intentionally avoids any HTTP/CLI wiring in Task 003. Wave 2 will implement the
//! transport and storage adapters and then integrate them with the existing HTTP/CLI surface.

pub mod app;
pub mod http_rpc;
pub mod network;
pub mod network_http;
pub mod node;
pub mod runtime;
pub mod storage;
pub mod types;

pub use node::{BindInfo, RaftNode};
pub use types::{NodeId, NodeMeta, TypeConfig};

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::{BindInfo, RaftNode};

    #[test]
    fn create_raft_node_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bind = BindInfo {
            raft_bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 60001),
            api_bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 60002),
        };

        let node = RaftNode::new(dir.path(), bind).expect("RaftNode::new");

        assert!(node.paths().wal_dir.is_dir());
        assert!(node.paths().snapshot_dir.is_dir());
    }
}
