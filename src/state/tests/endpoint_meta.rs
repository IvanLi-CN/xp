use super::*;

use pretty_assertions::assert_eq;

#[test]
fn upsert_vless_endpoint_manual_accepts_tcp_prefixed_dest() {
    let mut state = PersistedState::empty();
    let endpoint_id = "endpoint_1".to_string();
    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: "tcp://origin.example.test:443".to_string(),
            server_names: vec!["cdn-a.example.test".to_string()],
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: "chrome".to_string(),
        },
        reality_keys: RealityKeys {
            private_key: "priv".to_string(),
            public_key: "pub".to_string(),
        },
        short_ids: vec!["aaaaaaaaaaaaaaaa".to_string()],
        active_short_id: "aaaaaaaaaaaaaaaa".to_string(),
        canary_upstream: None,
        accepted_authorities: Vec::new(),
        mihomo_smux: Default::default(),
        managed_default: false,
    };
    let endpoint = Endpoint {
        endpoint_id: endpoint_id.clone(),
        node_id: "node_1".to_string(),
        tag: "vless-test".to_string(),
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
    assert_eq!(meta.reality.dest, "tcp://origin.example.test:443");
}
