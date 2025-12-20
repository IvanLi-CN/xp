use std::fmt::Debug;

use crate::raft::types::NodeId;

/// WAL (write-ahead log) boundary.
///
/// This maps to OpenRaft's log store (`RaftLogStorage`) in Wave 2.
pub trait WalLogStore: Send + Sync + Debug + 'static {
    /// Identify which node's local log this store belongs to (useful for metrics/logging).
    fn node_id(&self) -> NodeId;

    /// Append opaque log bytes.
    ///
    /// Task 003 uses an opaque representation to keep this boundary independent from OpenRaft's
    /// internal entry encoding. Wave 2 will choose the concrete encoding strategy.
    fn append(
        &self,
        _bytes: Vec<u8>,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
