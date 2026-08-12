use super::*;

#[test]
fn desired_inbound_hash_changes_for_vless_transport() {
    let mut endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
        node_id: xp_test_fixtures::label_node1().to_owned(),
        tag: xp_test_fixtures::label_vless_test().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::json!({
            "reality": xp_test_fixtures::endpoint_reality(),
            "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
            "short_ids": xp_test_fixtures::endpoint_short_ids(),
            "active_short_id": xp_test_fixtures::endpoint_active_short_id()
        }),
    };
    let vision_hash = desired_inbound_hash(&endpoint).unwrap();
    endpoint.meta["transport"] = serde_json::json!("xhttp");
    assert_ne!(desired_inbound_hash(&endpoint).unwrap(), vision_hash);
}
