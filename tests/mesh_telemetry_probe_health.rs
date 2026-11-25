use xp::mesh_telemetry::{
    DirectConnectivity, MeshPeerTelemetry, MeshTelemetryHandle, PublicFallbackHealth,
};

#[tokio::test]
async fn probe_health_fields_are_independent_and_persisted() {
    let temp = tempfile::tempdir().unwrap();
    let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
    telemetry
        .set_direct_connectivity("peer-a", "alpha", DirectConnectivity::Unavailable)
        .await
        .unwrap();
    telemetry
        .set_public_fallback_health("peer-a", "alpha", PublicFallbackHealth::Available)
        .await
        .unwrap();

    let peer = telemetry.snapshot().await.peers.remove(0);
    assert_eq!(peer.direct_connectivity, DirectConnectivity::Unavailable);
    assert_eq!(peer.public_fallback_health, PublicFallbackHealth::Available);
    assert_eq!(peer.buckets.len(), 0);

    let reloaded = MeshTelemetryHandle::load(temp.path()).unwrap();
    let peer = reloaded.snapshot().await.peers.remove(0);
    assert_eq!(peer.direct_connectivity, DirectConnectivity::Unavailable);
    assert_eq!(peer.public_fallback_health, PublicFallbackHealth::Available);
}

#[test]
fn legacy_peer_json_defaults_probe_fields_to_unknown() {
    let peer: MeshPeerTelemetry =
        serde_json::from_str(r#"{"peer_id":"peer-a","peer_name":"alpha"}"#).unwrap();
    assert_eq!(peer.direct_connectivity, DirectConnectivity::Unknown);
    assert_eq!(peer.public_fallback_health, PublicFallbackHealth::Unknown);
}
