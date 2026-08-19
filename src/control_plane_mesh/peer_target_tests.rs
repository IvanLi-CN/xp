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

fn managed_vless_endpoint(_endpoint_id: &str, port: u16) -> Endpoint {
    Endpoint {
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
    }
}

#[test]
fn peer_target_uses_mesh_only_for_one_managed_default_endpoint() {
    let node = peer_node();
    let unique = peer_target_from_node(&node, &[managed_vless_endpoint("one", 443)]);
    assert_eq!(
        unique.mesh_base_url.as_deref(),
        Some(
            format!(
                "https://{}:443",
                xp_test_fixtures::label_peer_afixture_test()
            )
            .as_str()
        )
    );
    assert_eq!(unique.mesh_reason, MeshPeerReason::MeshAvailable);
    assert_eq!(unique.public_base_url, node.api_base_url);
    let missing = peer_target_from_node(&node, &[]);
    assert!(missing.mesh_base_url.is_none());
    assert_eq!(missing.mesh_reason, MeshPeerReason::MissingEndpoint);
    let missing_access_host = Node {
        access_host: xp_test_fixtures::label_empty().to_owned(),
        ..node.clone()
    };
    assert!(
        peer_target_from_node(&missing_access_host, &[managed_vless_endpoint("one", 443)],)
            .mesh_base_url
            .is_none()
    );
    assert_eq!(
        peer_target_from_node(&missing_access_host, &[managed_vless_endpoint("one", 443)])
            .mesh_reason,
        MeshPeerReason::InvalidAccessHost
    );
    let invalid_access_host = Node {
        access_host: xp_test_fixtures::address_loopback().to_owned(),
        ..node.clone()
    };
    assert!(
        peer_target_from_node(&invalid_access_host, &[managed_vless_endpoint("one", 443)],)
            .mesh_base_url
            .is_none()
    );
    assert_eq!(
        peer_target_from_node(&invalid_access_host, &[managed_vless_endpoint("one", 443)])
            .mesh_reason,
        MeshPeerReason::InvalidAccessHost
    );
    let absolute_fqdn = Node {
        access_host: xp_test_fixtures::label_peer_afixture_test_variant2().to_owned(),
        ..node.clone()
    };
    assert_eq!(
        peer_target_from_node(&absolute_fqdn, &[managed_vless_endpoint("one", 443)],)
            .mesh_base_url
            .as_deref(),
        Some(
            format!(
                "https://{}:443",
                xp_test_fixtures::label_peer_afixture_test()
            )
            .as_str()
        )
    );
    let ambiguous = peer_target_from_node(
        &node,
        &[
            managed_vless_endpoint("one", 443),
            managed_vless_endpoint("two", 8443),
        ],
    );
    assert!(ambiguous.mesh_base_url.is_none());
    assert_eq!(ambiguous.mesh_reason, MeshPeerReason::AmbiguousEndpoint);
}

#[tokio::test]
async fn reverse_only_request_respects_the_local_readiness_gate() {
    let gate = Arc::new(AtomicBool::new(false));
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_reverse_gate(gate);
    let error = client
        .send_peer_reverse_request(
            &peer_target_from_node(&peer_node(), &[managed_vless_endpoint("one", 443)]),
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_string(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(xp_test_fixtures::number_value1()),
                allow_ambiguous_fallback: true,
                request_id: xp_test_fixtures::primary_node_id().to_owned(),
                route: InternalRoute::MeshV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::secondary_node_id().to_owned(),
                updates_active_path: true,
            },
            "",
            "",
        )
        .await
        .expect_err("disabled Reverse must not resolve or send a route");

    assert!(error.to_string().contains("reverse relay is disabled"));
}
