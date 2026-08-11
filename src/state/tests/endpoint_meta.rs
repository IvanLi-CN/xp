use super::*;

use crate::protocol::VlessRealityTransport;
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
        short_ids: xp_test_fixtures::endpoint_short_ids(),
        active_short_id: xp_test_fixtures::endpoint_active_short_id().to_owned(),
        canary_upstream: xp_test_fixtures::none(),
        accepted_authorities: xp_test_fixtures::host_list_empty(),
        mihomo_smux: Default::default(),
        transport: Default::default(),
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
    assert_eq!(meta.transport, VlessRealityTransport::VisionTcp);
    assert!(saved.meta.get("transport").is_none());
}

#[test]
fn build_new_vless_endpoint_meta_defaults_to_xhttp() {
    let value = build_endpoint_meta(
        &EndpointKind::VlessRealityVisionTcp,
        serde_json::json!({
            "reality": {
                "dest": xp_test_fixtures::url_tcp_origin_fixture_test443(),
                "server_names": xp_test_fixtures::host_list_edge34(),
                "server_names_source": "manual",
                "fingerprint": "chrome"
            }
        }),
    )
    .unwrap();

    let meta: VlessRealityVisionTcpEndpointMeta = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(meta.transport, VlessRealityTransport::Xhttp);
    assert_eq!(value["transport"], "xhttp");
}
