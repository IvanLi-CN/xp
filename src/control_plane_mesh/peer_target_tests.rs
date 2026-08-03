use super::*;

fn peer_node() -> Node {
    Node {
        node_id: "peer-a".to_string(),
        node_name: "peer-a".to_string(),
        access_host: "peer-a.example.test".to_string(),
        api_base_url: "https://public-peer-a.example.test".to_string(),
        quota_limit_bytes: 0,
        quota_reset: Default::default(),
    }
}

fn managed_vless_endpoint(endpoint_id: &str, port: u16) -> Endpoint {
    Endpoint {
        endpoint_id: endpoint_id.to_string(),
        node_id: "peer-a".to_string(),
        tag: format!("vless-vision-{endpoint_id}"),
        kind: crate::domain::EndpointKind::VlessRealityVisionTcp,
        port,
        meta: serde_json::json!({
            "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
            },
            "reality_keys": {
                "private_key": "private",
                "public_key": "public"
            },
            "short_ids": ["0123456789abcdef"],
            "active_short_id": "0123456789abcdef",
            "managed_default": true
        }),
    }
}

#[test]
fn peer_target_uses_mesh_only_for_one_managed_default_endpoint() {
    let node = peer_node();
    let unique = peer_target_from_node(&node, &[managed_vless_endpoint("one", 443)]);
    assert_eq!(
        unique.mesh_base_url.as_deref(),
        Some("https://peer-a.example.test:443")
    );
    assert_eq!(unique.public_base_url, node.api_base_url);
    assert!(peer_target_from_node(&node, &[]).mesh_base_url.is_none());
    let missing_access_host = Node {
        access_host: String::new(),
        ..node.clone()
    };
    assert!(
        peer_target_from_node(&missing_access_host, &[managed_vless_endpoint("one", 443)],)
            .mesh_base_url
            .is_none()
    );
    let invalid_access_host = Node {
        access_host: "https://peer-a.example.test:443/mesh".to_string(),
        ..node.clone()
    };
    assert!(
        peer_target_from_node(&invalid_access_host, &[managed_vless_endpoint("one", 443)],)
            .mesh_base_url
            .is_none()
    );
    let absolute_fqdn = Node {
        access_host: "peer-a.example.test.".to_string(),
        ..node.clone()
    };
    assert_eq!(
        peer_target_from_node(&absolute_fqdn, &[managed_vless_endpoint("one", 443)],)
            .mesh_base_url
            .as_deref(),
        Some("https://peer-a.example.test:443")
    );
    let ambiguous = peer_target_from_node(
        &node,
        &[
            managed_vless_endpoint("one", 443),
            managed_vless_endpoint("two", 8443),
        ],
    );
    assert!(ambiguous.mesh_base_url.is_none());
}
