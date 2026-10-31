use super::*;

impl MeshTelemetryHandle {
    /// Visit every retained bucket without cloning the full telemetry history.
    pub(crate) async fn visit_history_buckets<F>(&self, mut visit: F) -> anyhow::Result<()>
    where
        F: FnMut(&str, &MeshTelemetryBucket) -> anyhow::Result<()>,
    {
        let state = self.state.lock().await;
        for peer in state.persisted.peers.values() {
            for bucket in &peer.buckets {
                visit(&peer.peer_id, bucket)?;
            }
        }
        Ok(())
    }
}
