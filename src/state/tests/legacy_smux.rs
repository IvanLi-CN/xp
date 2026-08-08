use super::*;

#[test]
fn upsert_vless_endpoint_preserves_missing_legacy_smux_field() {
    let mut state = PersistedState::empty();
    let mut endpoint = vless_endpoint("endpoint_legacy", "node_1");
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
            .get("endpoint_legacy")
            .and_then(|saved| saved.meta.get("mihomo_smux"))
            .is_none()
    );
}

#[test]
fn reality_domain_update_preserves_missing_legacy_smux_field() {
    let mut state = PersistedState::empty();
    state
        .nodes
        .insert("node_1".to_string(), test_node("node_1"));
    let mut endpoint = vless_endpoint("endpoint_global_legacy", "node_1");
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
            server_name: "global.example.com".to_string(),
            disabled_node_ids: BTreeSet::new(),
        },
    }
    .apply(&mut state)
    .unwrap();

    assert!(
        state
            .endpoints
            .get("endpoint_global_legacy")
            .unwrap()
            .meta
            .get("mihomo_smux")
            .is_none()
    );
}
