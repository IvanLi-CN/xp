use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;

/// Binding info for the Raft-facing internal endpoint and the admin/API endpoint.
///
/// Task 003 only stores this information; it does not start listeners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindInfo {
    /// Raft RPC listener bind address (leader election, log replication, snapshot, etc.).
    pub raft_bind: SocketAddr,
    /// Admin/API listener bind address (used by humans and follower->leader forwarding).
    pub api_bind: SocketAddr,
}

/// Directory layout under the node's data dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaftPaths {
    pub root: PathBuf,
    pub wal_dir: PathBuf,
    pub snapshot_dir: PathBuf,
}

impl RaftPaths {
    pub fn new(data_dir: &Path) -> Self {
        let root = data_dir.join("raft");
        let wal_dir = root.join("wal");
        let snapshot_dir = root.join("snapshots");
        Self {
            root,
            wal_dir,
            snapshot_dir,
        }
    }
}

/// A wiring object representing the local Raft node process.
///
/// This struct is deliberately small and stable: Wave 2 will attach a transport implementation
/// and OpenRaft storage adapters without changing the constructor contract.
#[derive(Debug)]
pub struct RaftNode {
    data_dir: PathBuf,
    bind: BindInfo,
    paths: RaftPaths,
}

impl RaftNode {
    /// Construct a Raft node wiring object and ensure the on-disk directory layout exists.
    ///
    /// - `data_dir`: node data dir (will create `data_dir/raft/{wal,snapshots}`).
    /// - `bind`: Raft + API bind addresses (stored only; no networking is started).
    pub fn new(data_dir: impl Into<PathBuf>, bind: BindInfo) -> anyhow::Result<Self> {
        let data_dir = data_dir.into();
        let paths = RaftPaths::new(&data_dir);

        std::fs::create_dir_all(&paths.wal_dir)
            .with_context(|| format!("create wal dir: {}", paths.wal_dir.display()))?;
        std::fs::create_dir_all(&paths.snapshot_dir)
            .with_context(|| format!("create snapshot dir: {}", paths.snapshot_dir.display()))?;

        Ok(Self {
            data_dir,
            bind,
            paths,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn bind(&self) -> BindInfo {
        self.bind
    }

    pub fn paths(&self) -> &RaftPaths {
        &self.paths
    }
}
