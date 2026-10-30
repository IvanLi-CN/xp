use super::*;

pub(super) fn test_vless_meta(managed_default: bool) -> Value {
    serde_json::to_value(VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: xp_test_fixtures::address_loopback_port39531().to_owned(),
            server_names: xp_test_fixtures::host_list_edge37(),
            server_names_source: Default::default(),
            fingerprint: "chrome".to_string(),
        },
        reality_keys: RealityKeys {
            private_key: "private".to_string(),
            public_key: "public".to_string(),
        },
        short_ids: xp_test_fixtures::endpoint_short_ids(),
        active_short_id: xp_test_fixtures::endpoint_active_short_id().to_owned(),
        canary_upstream: xp_test_fixtures::none(),
        accepted_authorities: xp_test_fixtures::host_list_empty(),
        mihomo_xmux: None,
        mihomo_smux: Default::default(),
        transport: Default::default(),
        managed_default,
    })
    .unwrap()
}
