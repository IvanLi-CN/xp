use std::time::Instant;

use super::{ActiveRouteKind, MeshActiveRoute, MeshTelemetryHandle, MeshTelemetrySample};

#[derive(Debug, Clone)]
pub struct ReverseRelayTelemetrySample {
    pub peer_id: String,
    pub peer_name: String,
    pub rendezvous: String,
    pub rendezvous_role: String,
    pub primary_rendezvous: String,
    pub standby_rendezvous: Option<String>,
    pub generation: u64,
    pub sample: MeshTelemetrySample,
}

impl MeshTelemetryHandle {
    pub async fn record_reverse_sample(
        &self,
        reverse_sample: ReverseRelayTelemetrySample,
    ) -> anyhow::Result<()> {
        let ReverseRelayTelemetrySample {
            peer_id,
            peer_name,
            rendezvous,
            rendezvous_role,
            primary_rendezvous,
            standby_rendezvous,
            generation,
            sample,
        } = reverse_sample;
        self.record_sample(peer_id.clone(), peer_name, sample)
            .await?;
        let mut state = self.state.lock().await;
        if let Some(peer) = state.persisted.peers.get_mut(&peer_id) {
            peer.active_route = Some(MeshActiveRoute {
                kind: ActiveRouteKind::ReverseRelay,
                rendezvous: Some(rendezvous),
                rendezvous_role: Some(rendezvous_role),
                primary_rendezvous: Some(primary_rendezvous),
                standby_rendezvous,
                generation: Some(generation),
                readiness: Some("active".to_string()),
            });
            state.persisted.revision += 1;
            self.persist_immediately(&mut state, Instant::now())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_telemetry::TelemetryPath;

    #[tokio::test]
    async fn reverse_sample_persists_active_and_standby_rendezvous() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        telemetry
            .record_reverse_sample(ReverseRelayTelemetrySample {
                peer_id: "target-a".to_string(),
                peer_name: "target-a".to_string(),
                rendezvous: "rendezvous-b".to_string(),
                rendezvous_role: "primary".to_string(),
                primary_rendezvous: "rendezvous-b".to_string(),
                standby_rendezvous: Some("rendezvous-c".to_string()),
                generation: 7,
                sample: MeshTelemetrySample {
                    path: TelemetryPath::Mesh,
                    success: true,
                    latency_ms: Some(xp_test_fixtures::number_value42()),
                    fallback: false,
                    updates_active_path: true,
                    transport: None,
                },
            })
            .await
            .unwrap();

        let restored = MeshTelemetryHandle::load(temp.path()).unwrap();
        let route = restored
            .snapshot()
            .await
            .peers
            .remove(0)
            .active_route
            .unwrap();
        assert_eq!(route.kind, ActiveRouteKind::ReverseRelay);
        assert_eq!(route.rendezvous.as_deref(), Some("rendezvous-b"));
        assert_eq!(route.rendezvous_role.as_deref(), Some("primary"));
        assert_eq!(route.primary_rendezvous.as_deref(), Some("rendezvous-b"));
        assert_eq!(route.standby_rendezvous.as_deref(), Some("rendezvous-c"));
        assert_eq!(route.generation, Some(7));
    }
}
