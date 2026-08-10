use super::*;

use pretty_assertions::assert_eq;

#[test]
fn upsert_vless_endpoint_manual_accepts_tcp_prefixed_dest() {
    let mut state = PersistedState::empty();
    let endpoint_id = xp_test_fixtures::label_endpoint1().to_owned();
    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: xp_test_fixtures::url_tcp_origin_fixture_test443().to_owned(),
            server_names: xp_test_fixtures::host_list_edge34(),
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: "chrome".to_string(),
        },
        reality_keys: RealityKeys {
            private_key: "priv".to_string(),
            public_key: "pub".to_string(),
        },
        short_ids: vec!["aaaaaaaaaaaaaaaa".to_string()],
        active_short_id: "aaaaaaaaaaaaaaaa".to_string(),
        canary_upstream: xp_test_fixtures::none(),
        accepted_authorities: xp_test_fixtures::host_list_empty(),
        mihomo_smux: Default::default(),
        managed_default: false,
    };
    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
        node_id: xp_test_fixtures::label_node1().to_owned(),
        tag: xp_test_fixtures::label_vless_test().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::to_value(meta).unwrap(),
    };

    DesiredStateCommand::UpsertEndpoint { endpoint }
        .apply(&mut state)
        .unwrap();
    let saved = state.endpoints.get(&endpoint_id).unwrap();
    let meta: VlessRealityVisionTcpEndpointMeta =
        serde_json::from_value(saved.meta.clone()).expect("vless meta");
    assert_eq!(
        meta.reality.dest,
        xp_test_fixtures::url_tcp_origin_fixture_test443()
    );
}
