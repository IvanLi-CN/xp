use std::fmt::Debug;

/// Snapshot builder boundary.
///
/// A snapshot is the serialized full desired-state + metadata (last applied log id, membership, etc.).
/// Wave 2 will make this produce a file or a stream and install it atomically with log truncation.
pub trait SnapshotBuilder: Send + Sync + Debug + 'static {
    fn build_snapshot(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<u8>>> + Send;
}

/// Snapshot store boundary.
pub trait SnapshotStore: Send + Sync + Debug + 'static {
    fn install_snapshot(
        &self,
        _snapshot_bytes: Vec<u8>,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

