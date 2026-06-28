use super::*;

fn endpoint_vless(
    endpoint_id: &str,
    port: u16,
    server_names: &[&str],
    managed_default: Option<bool>,
) -> Endpoint {
    let mut meta = serde_json::json!({
        "reality": {
            "dest": "example.com:443",
            "server_names": server_names,
            "fingerprint": "chrome"
        },
        "reality_keys": {
            "private_key": "private",
            "public_key": "public"
        },
        "short_ids": ["0123456789abcdef"],
        "active_short_id": "0123456789abcdef"
    });
    if let Some(value) = managed_default {
        meta["managed_default"] = serde_json::Value::Bool(value);
    }
    Endpoint {
        endpoint_id: endpoint_id.to_string(),
        node_id: "n1".to_string(),
        tag: format!("vless-vision-{endpoint_id}"),
        kind: EndpointKind::VlessRealityVisionTcp,
        port,
        meta,
    }
}

fn endpoint_ss(endpoint_id: &str, port: u16, managed_default: Option<bool>) -> Endpoint {
    let mut meta = serde_json::json!({
        "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
        "server_psk_b64": "AAAAAAAAAAAAAAAAAAAAAA=="
    });
    if let Some(value) = managed_default {
        meta["managed_default"] = serde_json::Value::Bool(value);
    }
    Endpoint {
        endpoint_id: endpoint_id.to_string(),
        node_id: "n1".to_string(),
        tag: format!("ss2022-{endpoint_id}"),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port,
        meta,
    }
}

#[test]
fn build_default_vless_endpoint_spec_rejects_zero_port() {
    let err = build_default_vless_endpoint_spec(
        Some(0),
        "node.example.com",
        Some("public.sn.files.1drv.com"),
        None,
        "127.0.0.1:39043".parse().unwrap(),
    )
    .unwrap_err();

    assert!(err.to_string().contains("invalid port: 0"));
}

#[test]
fn build_default_ss_endpoint_spec_rejects_zero_port() {
    let err = build_default_ss_endpoint_spec(Some(0)).unwrap_err();
    assert!(err.to_string().contains("invalid port: 0"));
}

#[tokio::test]
async fn explicit_vless_spec_adopts_single_legacy_vless_and_rewrites_canary_dest() {
    let tempdir = tempfile::tempdir().unwrap();
    let endpoint = endpoint_vless("e1", 53844, &["example.com"], None);
    let mut writes = Vec::<DesiredStateCommand>::new();
    let spec = ManagedDefaultEndpointsSpec {
        vless: Some(DefaultVlessEndpointSpec {
            port: 53844,
            reality_dest: "127.0.0.1:39043".to_string(),
            server_names: vec!["example.com".to_string()],
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: "chrome".to_string(),
        }),
        ss: None,
    };
    let bind = "127.0.0.1:39043".parse().unwrap();

    {
        let mut writer = |cmd| {
            writes.push(cmd);
            std::future::ready(Ok(()))
        };
        reconcile_host_managed_default_endpoints(
            tempdir.path(),
            "n1",
            &[endpoint],
            HostManagedDefaultEndpointsOptions {
                explicit: &spec,
                access_host: "node.example.com",
                vless_canary_bind: bind,
            },
            &mut writer,
            "test",
        )
        .await
        .unwrap();
    }

    assert_eq!(writes.len(), 1);
    match &writes[0] {
        DesiredStateCommand::UpsertEndpoint { endpoint } => {
            let meta: VlessRealityVisionTcpEndpointMeta =
                serde_json::from_value(endpoint.meta.clone()).unwrap();
            assert!(meta.managed_default);
            assert_eq!(meta.reality.dest, "127.0.0.1:39043");
            assert_eq!(meta.reality.server_names, vec!["example.com"]);
            assert_eq!(endpoint.port, 53844);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn host_managed_legacy_vless_is_auto_adopted_without_explicit_config() {
    let endpoint = endpoint_vless("e1", 53844, &["example.com"], None);
    let spec = resolve_host_managed_default_endpoints_spec(
        &ManagedDefaultEndpointsSpec::default(),
        &[endpoint],
        "node.example.com",
        "127.0.0.1:39043".parse().unwrap(),
    )
    .unwrap();

    let vless = spec
        .vless
        .expect("legacy VLESS endpoint should be auto-adopted");
    assert_eq!(vless.port, 53844);
    assert_eq!(vless.reality_dest, "127.0.0.1:39043");
    assert_eq!(vless.server_names, vec!["node.example.com"]);
    assert_eq!(vless.server_names_source, RealityServerNamesSource::Manual);
    assert!(spec.ss.is_none());
}

#[test]
fn host_managed_vless_with_false_flag_is_not_auto_adopted() {
    let endpoint = endpoint_vless("e1", 53844, &["example.com"], Some(false));
    let spec = resolve_host_managed_default_endpoints_spec(
        &ManagedDefaultEndpointsSpec::default(),
        &[endpoint],
        "node.example.com",
        "127.0.0.1:39043".parse().unwrap(),
    )
    .unwrap();

    assert!(spec.vless.is_none());
    assert!(spec.ss.is_none());
}

#[test]
fn host_managed_multiple_legacy_vless_are_not_auto_adopted() {
    let endpoints = vec![
        endpoint_vless("e1", 53844, &["example.com"], None),
        endpoint_vless("e2", 53845, &["example.org"], None),
    ];
    let spec = resolve_host_managed_default_endpoints_spec(
        &ManagedDefaultEndpointsSpec::default(),
        &endpoints,
        "node.example.com",
        "127.0.0.1:39043".parse().unwrap(),
    )
    .unwrap();

    assert!(spec.vless.is_none());
    assert!(spec.ss.is_none());
}

#[test]
fn host_managed_explicitly_cleared_vless_is_not_rederived_from_marked_endpoint() {
    let endpoint = endpoint_vless("e1", 53844, &["example.com"], Some(true));
    let state = ManagedDefaultEndpointsState {
        schema_version: MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION,
        vless_endpoint_id: Some("e1".to_string()),
        vless_source: Some(ManagedDefaultEndpointSource::Explicit),
        ss_endpoint_id: None,
        ss_source: None,
    };
    let intent = resolve_host_managed_default_endpoints_intent(
        &ManagedDefaultEndpointsSpec::default(),
        &[endpoint],
        "node.example.com",
        "127.0.0.1:39043".parse().unwrap(),
        &state,
    )
    .unwrap();

    assert!(matches!(intent.vless, ManagedDefaultEndpointIntent::Remove));
}

#[test]
fn host_managed_auto_adopted_vless_preserves_global_server_name_mode() {
    let mut endpoint = endpoint_vless("e1", 53844, &["example.com"], Some(true));
    endpoint.meta["reality"]["server_names_source"] =
        serde_json::Value::String("global".to_string());
    let spec = resolve_host_managed_default_endpoints_spec(
        &ManagedDefaultEndpointsSpec::default(),
        &[endpoint],
        "node.example.com",
        "127.0.0.1:39043".parse().unwrap(),
    )
    .unwrap();

    let vless = spec
        .vless
        .expect("legacy VLESS endpoint should be auto-adopted");
    assert_eq!(vless.server_names_source, RealityServerNamesSource::Manual);
    assert_eq!(vless.reality_dest, "127.0.0.1:39043");
}

#[test]
fn host_managed_auto_adopted_vless_keeps_manage_intent_without_explicit_config() {
    let endpoint = endpoint_vless("e1", 53844, &["example.com"], Some(true));
    let state = ManagedDefaultEndpointsState {
        schema_version: MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION,
        vless_endpoint_id: Some("e1".to_string()),
        vless_source: Some(ManagedDefaultEndpointSource::AutoAdopted),
        ss_endpoint_id: None,
        ss_source: None,
    };
    let intent = resolve_host_managed_default_endpoints_intent(
        &ManagedDefaultEndpointsSpec::default(),
        &[endpoint],
        "node.example.com",
        "127.0.0.1:39043".parse().unwrap(),
        &state,
    )
    .unwrap();

    assert!(matches!(
        intent.vless,
        ManagedDefaultEndpointIntent::Manage {
            source: ManagedDefaultEndpointSource::AutoAdopted,
            ..
        }
    ));
}

#[tokio::test]
async fn persists_adopted_endpoint_ids_before_later_kind_fails() {
    let tempdir = tempfile::tempdir().unwrap();
    let endpoints = vec![
        endpoint_vless("e1", 53844, &["example.com"], None),
        endpoint_ss("s1", 443, None),
        endpoint_ss("s2", 8443, None),
    ];
    let spec = ManagedDefaultEndpointsSpec {
        vless: Some(DefaultVlessEndpointSpec {
            port: 53844,
            reality_dest: "127.0.0.1:39043".to_string(),
            server_names: vec!["example.com".to_string()],
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: "chrome".to_string(),
        }),
        ss: Some(DefaultSsEndpointSpec { port: 9443 }),
    };
    let mut writes = Vec::<DesiredStateCommand>::new();

    let err = {
        let mut writer = |cmd| {
            writes.push(cmd);
            std::future::ready(Ok(()))
        };
        let intent = ManagedDefaultEndpointsIntent {
            vless: ManagedDefaultEndpointIntent::Manage {
                spec: spec.vless.clone().unwrap(),
                source: ManagedDefaultEndpointSource::AutoAdopted,
            },
            ss: ManagedDefaultEndpointIntent::Manage {
                spec: spec.ss.clone().unwrap(),
                source: ManagedDefaultEndpointSource::Explicit,
            },
        };
        reconcile_managed_default_endpoints(
            tempdir.path(),
            "n1",
            &endpoints,
            &intent,
            &mut writer,
            "test",
        )
        .await
        .expect_err("ss ambiguity should still fail after vless adoption")
    };

    assert!(
        err.to_string()
            .contains("multiple ss2022_2022_blake3_aes_128_gcm endpoints already exist")
    );
    assert_eq!(writes.len(), 1);
    let state = load_managed_default_endpoints_state(tempdir.path()).unwrap();
    assert_eq!(state.vless_endpoint_id.as_deref(), Some("e1"));
    assert_eq!(state.ss_endpoint_id, None);
}
