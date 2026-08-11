use super::*;

#[test]
fn upsert_vless_endpoint_preserves_missing_legacy_optional_fields() {
    let mut state = PersistedState::empty();
    let endpoint_id = xp_test_fixtures::label_vless1();
    let mut endpoint = vless_endpoint("vless_1", xp_test_fixtures::label_node1());
    endpoint
        .meta
        .as_object_mut()
        .expect("vless metadata is an object")
        .remove("mihomo_smux");
    endpoint.meta.as_object_mut().unwrap().remove("transport");

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
    assert!(state.endpoints[endpoint_id].meta.get("transport").is_none());
}

#[test]
fn reality_domain_update_preserves_missing_legacy_optional_fields() {
    let mut state = PersistedState::empty();
    let node_id = xp_test_fixtures::label_node1();
    let endpoint_id = xp_test_fixtures::label_vless2();
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
    object.remove("transport");
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
    assert!(state.endpoints[endpoint_id].meta.get("transport").is_none());
}

#[test]
fn short_id_rotation_preserves_missing_legacy_optional_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();

    let endpoint_id = xp_test_fixtures::label_endpoint1().to_owned();
    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: xp_test_fixtures::address_loopback_port39514().to_owned(),
            server_names: xp_test_fixtures::host_list_edge31(),
            server_names_source: Default::default(),
            fingerprint: "chrome".to_string(),
        },
        reality_keys: RealityKeys {
            private_key: "priv".to_string(),
            public_key: "pub".to_string(),
        },
        short_ids: xp_test_fixtures::endpoint_short_ids(),
        active_short_id: xp_test_fixtures::endpoint_active_short_id().to_owned(),
        canary_upstream: xp_test_fixtures::none(),
        accepted_authorities: xp_test_fixtures::host_list_empty(),
        mihomo_smux: Default::default(),
        transport: Default::default(),
        managed_default: false,
    };
    let mut endpoint_meta = serde_json::to_value(meta).unwrap();
    endpoint_meta.as_object_mut().unwrap().remove("mihomo_smux");
    store.state_mut().endpoints.insert(
        endpoint_id.clone(),
        Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
            node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
            tag: xp_test_fixtures::label_ss2().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 443,
            meta: endpoint_meta,
        },
    );
    store.save().unwrap();

    let mut rng = StdRng::seed_from_u64(42);
    let out = store
        .rotate_vless_reality_short_id_with_rng(&endpoint_id, &mut rng)
        .unwrap()
        .unwrap();
    drop(store);

    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let endpoint = store.get_endpoint(&endpoint_id).unwrap();
    let meta: VlessRealityVisionTcpEndpointMeta =
        serde_json::from_value(endpoint.meta.clone()).unwrap();
    pretty_assertions::assert_eq!(out.active_short_id, meta.active_short_id);
    pretty_assertions::assert_eq!(out.short_ids, meta.short_ids);
    assert!(endpoint.meta.get("mihomo_smux").is_none());
    assert!(endpoint.meta.get("transport").is_none());
}
