use super::*;

#[test]
fn upsert_vless_endpoint_preserves_missing_legacy_smux_field() {
    let mut state = PersistedState::empty();
    let endpoint_id = xp_test_fixtures::slot_s467();
    let mut endpoint = vless_endpoint("vless_1", xp_test_fixtures::slot_s477());
    endpoint
        .meta
        .as_object_mut()
        .expect("vless metadata is an object")
        .remove("mihomo_smux");

    DesiredStateCommand::UpsertEndpoint { endpoint }
        .apply(&mut state)
        .unwrap();

    assert!(
        state
            .endpoints
            .get(endpoint_id)
            .and_then(|saved| saved.meta.get("mihomo_smux"))
            .is_none()
    );
}

#[test]
fn reality_domain_update_preserves_missing_legacy_smux_field() {
    let mut state = PersistedState::empty();
    let node_id = xp_test_fixtures::slot_s477();
    let endpoint_id = xp_test_fixtures::slot_s536();
    state.nodes.insert(node_id.to_owned(), test_node(node_id));
    let mut endpoint = vless_endpoint("vless_2", node_id);
    let object = endpoint.meta.as_object_mut().unwrap();
    object
        .get_mut("reality")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert(
            "server_names_source".to_string(),
            serde_json::Value::String("global".to_string()),
        );
    object.remove("mihomo_smux");
    state
        .endpoints
        .insert(endpoint.endpoint_id.clone(), endpoint);

    DesiredStateCommand::CreateRealityDomain {
        domain: crate::domain::RealityDomain {
            domain_id: "domain_1".to_string(),
            server_name: xp_test_fixtures::primary_server_name().to_owned(),
            disabled_node_ids: BTreeSet::new(),
        },
    }
    .apply(&mut state)
    .unwrap();

    assert!(
        state
            .endpoints
            .get(endpoint_id)
            .unwrap()
            .meta
            .get("mihomo_smux")
            .is_none()
    );
}
