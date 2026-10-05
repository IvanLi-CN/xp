use super::*;

#[derive(Default)]
pub(super) struct SampleWriteOptions {
    pub active_route: Option<MeshActiveRoute>,
    pub mesh_reason: Option<(Option<String>, MeshPeerReason)>,
    pub defer_persist: bool,
    pub deadline: Option<StdInstant>,
}

impl MeshTelemetryHandle {
    pub async fn record_sample(
        &self,
        peer_id: impl Into<String>,
        peer_name: impl Into<String>,
        sample: MeshTelemetrySample,
    ) -> anyhow::Result<()> {
        self.record_sample_with_active_route(
            peer_id,
            peer_name,
            sample,
            SampleWriteOptions::default(),
        )
        .await
        .map(|_| ())
    }

    #[cfg(test)]
    pub(crate) async fn record_sample_deferred(
        &self,
        peer_id: impl Into<String>,
        peer_name: impl Into<String>,
        sample: MeshTelemetrySample,
        mesh_reason: Option<(Option<String>, MeshPeerReason)>,
    ) -> anyhow::Result<()> {
        self.record_sample_with_active_route(
            peer_id,
            peer_name,
            sample,
            SampleWriteOptions {
                mesh_reason,
                defer_persist: true,
                ..SampleWriteOptions::default()
            },
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn record_sample_until(
        &self,
        peer_id: impl Into<String>,
        peer_name: impl Into<String>,
        sample: MeshTelemetrySample,
        deadline: StdInstant,
    ) -> anyhow::Result<bool> {
        self.record_sample_with_active_route(
            peer_id,
            peer_name,
            sample,
            SampleWriteOptions {
                deadline: Some(deadline),
                ..SampleWriteOptions::default()
            },
        )
        .await
    }

    pub(super) async fn record_sample_with_active_route(
        &self,
        peer_id: impl Into<String>,
        peer_name: impl Into<String>,
        sample: MeshTelemetrySample,
        options: SampleWriteOptions,
    ) -> anyhow::Result<bool> {
        let SampleWriteOptions {
            active_route,
            mesh_reason,
            defer_persist,
            deadline,
        } = options;
        if deadline.is_some_and(|deadline| StdInstant::now() >= deadline) {
            return Ok(false);
        }
        let now = Utc::now();
        let peer_id = peer_id.into();
        let (within_deadline, observed_transport) = if let Some(deadline) = deadline {
            self.connections
                .observe_until(&peer_id, sample.transport, deadline)
                .await
        } else {
            (
                true,
                self.connections.observe(&peer_id, sample.transport).await,
            )
        };
        if !within_deadline {
            return Ok(false);
        }
        let mut state = self.state.lock().await;
        if deadline.is_some_and(|deadline| StdInstant::now() >= deadline) {
            return Ok(false);
        }
        let peer = state
            .persisted
            .peers
            .entry(peer_id.clone())
            .or_insert_with(|| MeshPeerTelemetry {
                peer_id: peer_id.clone(),
                ..MeshPeerTelemetry::default()
            });
        peer.peer_name = peer_name.into();
        let previous_path = peer.last_path;
        let updates_active_path = sample.updates_active_path && sample.success;
        if updates_active_path {
            peer.last_path = Some(sample.path);
            let active_route = active_route.unwrap_or(MeshActiveRoute {
                kind: match sample.path {
                    TelemetryPath::Mesh => ActiveRouteKind::RealityDirect,
                    TelemetryPath::Public => ActiveRouteKind::Public,
                },
                rendezvous: None,
                rendezvous_role: None,
                primary_rendezvous: None,
                standby_rendezvous: None,
                generation: None,
                readiness: None,
            });
            peer.active_route = Some(active_route);
        }
        peer.last_sample_at = Some(timestamp(now));
        if updates_active_path && previous_path != Some(sample.path) {
            peer.last_transition_at = Some(timestamp(now));
        }
        if let Some(observed) = observed_transport {
            peer.last_mesh_protocol = Some(observed.protocol);
            if observed.connection_started {
                peer.connection_generation = peer.connection_generation.saturating_add(1);
                peer.last_connection_started_at = Some(timestamp(now));
            }
            if let Some(requests) = observed.current_connection_requests {
                peer.current_connection_requests = requests;
            }
        }
        if let Some((mesh_target, reason)) = mesh_reason {
            (peer.last_mesh_reason, peer.last_mesh_target) = (Some(reason), mesh_target);
        }
        let bucket = ensure_bucket(peer, now);
        match (sample.path, sample.success) {
            (TelemetryPath::Mesh, true) => {
                bucket.mesh_success += 1;
                bucket.end_to_end_success += 1;
            }
            (TelemetryPath::Mesh, false) => bucket.mesh_failure += 1,
            (TelemetryPath::Public, true) => {
                bucket.public_success += 1;
                bucket.end_to_end_success += 1;
            }
            (TelemetryPath::Public, false) => {
                bucket.public_failure += 1;
                bucket.end_to_end_failure += 1;
            }
        }
        if sample.fallback && sample.success {
            bucket.fallback_success += 1;
        }
        if let Some(observed) = observed_transport {
            if observed.protocol == MeshTransportProtocol::H2 {
                bucket.mesh_h2_requests = bucket.mesh_h2_requests.saturating_add(1);
            }
            if observed.connection_started {
                bucket.mesh_connection_starts = bucket.mesh_connection_starts.saturating_add(1);
            }
        }
        if let Some(latency_ms) = sample.latency_ms
            && bucket.latency_samples_ms.len() < 64
        {
            bucket.latency_samples_ms.push(latency_ms);
        }
        state.persisted.revision += 1;
        let deferred_flush = if defer_persist {
            self.schedule_deferred_flush(&mut state, Instant::now(), true)
        } else {
            self.persist_sample_if_due(&mut state, Instant::now())?
        };
        drop(state);
        if let Some(delay) = deferred_flush {
            self.spawn_deferred_flush(delay);
        }
        Ok(true)
    }
}
