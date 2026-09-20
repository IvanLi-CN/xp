use sha2::{Digest, Sha256};

use super::AdminMeshStatusResponse;

pub(super) fn mesh_status_etag(snapshot: &AdminMeshStatusResponse) -> String {
    let mut stable_snapshot = snapshot.clone();
    stable_snapshot.generated_at.clear();
    let stable_bytes = serde_json::to_vec(&stable_snapshot).expect("serialize mesh status ETag");
    format!("\"mesh-{}\"", hex::encode(Sha256::digest(stable_bytes)))
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::mesh_status_etag;
    use crate::mesh_telemetry::{MeshTransportHealth, MeshTransportProtocol};

    #[test]
    fn mesh_status_etag_tracks_stable_evidence_and_sampling_time() {
        fn response(generated_at: &str, connection_starts_5m: u32) -> AdminMeshStatusResponse {
            AdminMeshStatusResponse {
                generated_at: generated_at.to_string(),
                revision: 7,
                cluster_mesh_enabled: true,
                local: AdminMeshLocalStatus {
                    node_id: "local".to_string(),
                    node_name: "local".to_string(),
                    cluster_id: "cluster".to_string(),
                    role: "leader".to_string(),
                    leader_api_base_url: "https://local.example.test".to_string(),
                    term: 3,
                    canary: crate::vless_https_canary::VlessHttpsCanaryStatus::disabled(
                        std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                    ),
                    connection_usage: None,
                },
                peers: vec![AdminMeshPeerStatus {
                    node_id: "peer".to_string(),
                    node_name: "peer".to_string(),
                    api_base_url: "https://peer.example.test".to_string(),
                    mesh_url: Some("https://peer.example.test:443".to_string()),
                    mesh_capability: Some("enabled".to_string()),
                    mesh_reason: Some(crate::mesh_telemetry::MeshPeerReason::MeshAvailable),
                    current_path: Some(TelemetryPath::Mesh),
                    active_route: None,
                    quality: MeshQuality::Good,
                    stale: false,
                    breaker: BreakerState::Closed,
                    last_sample_at: None,
                    last_transition_at: None,
                    availability_1h: Some(1.0),
                    availability_24h: Some(1.0),
                    mesh_availability_24h: Some(1.0),
                    latency_p50_ms: Some(10),
                    latency_p95_ms: Some(20),
                    mesh_transport: Some(AdminMeshTransportStatus {
                        protocol: Some(MeshTransportProtocol::H2),
                        health: MeshTransportHealth::Healthy,
                        connection_generation: 2,
                        current_connection_requests: 12,
                        requests_5m: 12,
                        connection_starts_5m,
                        requests_1h: 60,
                        connection_starts_1h: 2,
                        last_connection_started_at: None,
                    }),
                    reverse_underlay: None,
                    buckets: Vec::new(),
                }],
                events: Vec::new(),
            }
        }
        let first = response("2026-08-08T10:00:00Z", 1);
        let generated_later = response("2026-08-08T10:01:00Z", 1);
        let churning = response("2026-08-08T10:01:00Z", 3);
        assert_eq!(mesh_status_etag(&first), mesh_status_etag(&generated_later));
        assert_ne!(mesh_status_etag(&first), mesh_status_etag(&churning));

        let mut sampled = first.clone();
        sampled.local.connection_usage = Some(AdminMeshConnectionUsage {
            supported: true,
            sampled_at: Some("2026-08-08T10:00:00Z".to_string()),
            warning: None,
            user_inbound: super::super::status::AdminUserInboundStatus {
                connections: Some(0),
                external: Some(0),
                cluster_peer: Some(0),
                unknown: Some(0),
                sources: Vec::new(),
                sources_truncated: false,
            },
        });
        let mut sampled_later = sampled.clone();
        sampled_later
            .local
            .connection_usage
            .as_mut()
            .expect("sampled usage")
            .sampled_at = Some("2026-08-08T10:01:00Z".to_string());
        assert_ne!(mesh_status_etag(&sampled), mesh_status_etag(&sampled_later));
    }
}
