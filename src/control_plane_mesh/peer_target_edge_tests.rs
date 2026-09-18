use super::*;

fn peer_node() -> Node {
    Node {
        node_id: xp_test_fixtures::label_peer_a().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        access_host: xp_test_fixtures::label_peer_afixture_test().to_owned(),
        api_base_url: xp_test_fixtures::url_https_public_peer_afixture_test().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: Default::default(),
    }
}

fn managed_xhttp_endpoint(port: u16) -> Endpoint {
    let mut endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::label_ss1().to_owned(),
        node_id: xp_test_fixtures::label_peer_a().to_owned(),
        tag: xp_test_fixtures::endpoint_tag_fixture507().to_owned(),
        kind: crate::domain::EndpointKind::VlessRealityVisionTcp,
        port,
        meta: serde_json::json!({
            "reality": xp_test_fixtures::endpoint_reality(),
            "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
            "short_ids": xp_test_fixtures::endpoint_short_ids(),
            "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
            "managed_default": true
        }),
    };
    endpoint.meta["transport"] = serde_json::json!("xhttp");
    endpoint
}

#[test]
fn peer_target_skips_xhttp_endpoint_for_control_plane_mesh() {
    let node = peer_node();
    let target = peer_target_from_node(&node, &[managed_xhttp_endpoint(443)]);
    assert!(target.mesh_base_url.is_none());
    assert_eq!(target.mesh_reason, MeshPeerReason::UnsupportedTransport);
    assert_eq!(target.public_base_url, node.api_base_url);
}
