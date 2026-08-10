use super::*;

fn endpoint_vless(
    endpoint_id: &str,
    port: u16,
    server_names: &[&str],
    managed_default: Option<bool>,
) -> Endpoint {
    let mut meta = serde_json::json!({
        "reality": xp_test_fixtures::endpoint_reality(),
        "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
        "short_ids": xp_test_fixtures::endpoint_short_ids(),
        "active_short_id": xp_test_fixtures::endpoint_active_short_id()
    });
    meta["reality"]["server_names"] = serde_json::json!(server_names);
    if let Some(value) = managed_default {
        meta["managed_default"] = serde_json::Value::Bool(value);
    }
    match endpoint_id {
        "e1" => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e1().to_owned(),
            node_id: xp_test_fixtures::label_n1().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_fixture507().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port,
            meta,
        },
        "e2" => Endpoint {
            endpoint_id: xp_test_fixtures::subscription_endpoint_e2().to_owned(),
            node_id: xp_test_fixtures::label_n1().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_fixture507().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port,
            meta,
        },
        _ => panic!("unknown VLESS endpoint fixture: {endpoint_id}"),
    }
}

fn endpoint_ss(endpoint_id: &str, port: u16, managed_default: Option<bool>) -> Endpoint {
    let mut meta = serde_json::json!({
        "method": SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
        "server_psk_b64": xp_test_fixtures::endpoint_server_psk_b64()
    });
    if let Some(value) = managed_default {
        meta["managed_default"] = serde_json::Value::Bool(value);
    }
    match endpoint_id {
        "s1" => Endpoint {
            endpoint_id: xp_test_fixtures::label_ss1().to_owned(),
            node_id: xp_test_fixtures::label_n1().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_fixture510().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta,
        },
        "s2" => Endpoint {
            endpoint_id: xp_test_fixtures::label_ss1().to_owned(),
            node_id: xp_test_fixtures::label_n1().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_fixture510().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            meta,
        },
        _ => panic!("unknown Shadowsocks endpoint fixture: {endpoint_id}"),
    }
}

#[test]
fn build_default_vless_endpoint_spec_rejects_zero_port() {
    let err = build_default_vless_endpoint_spec(
        Some(0),
        "node.example.com",
        Some("cdn-a.example.test"),
        None,
        "127.0.0.1:39043".parse().unwrap(),
    )
    .unwrap_err();

    assert!(err.to_string().contains("invalid port: 0"));
}

#[test]
fn build_default_vless_endpoint_spec_ignores_auxiliary_fields_without_bootstrap_port() {
    let spec = build_default_vless_endpoint_spec(
        None,
        "node.example.com",
        Some("cdn-a.example.test"),
        Some("chrome"),
        "127.0.0.1:39043".parse().unwrap(),
    )
    .unwrap();

    assert!(spec.is_none());
}

#[test]
fn build_default_ss_endpoint_spec_rejects_zero_port() {
    let err = build_default_ss_endpoint_spec(Some(0)).unwrap_err();
    assert!(err.to_string().contains("invalid port: 0"));
}

#[tokio::test]
async fn explicit_vless_spec_adopts_single_legacy_vless_and_rewrites_canary_dest() {
    let tempdir = tempfile::tempdir().unwrap();
    let endpoint = endpoint_vless("e1", 30445, &["example.com"], None);
    let mut writes = Vec::<DesiredStateCommand>::new();
    let spec = ManagedDefaultEndpointsSpec {
        vless: Some(DefaultVlessEndpointSpec {
            port: 30443,
            reality_dest: "127.0.0.1:39043".to_string(),
            server_names: xp_test_fixtures::host_list_edge31(),
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
                access_host: xp_test_fixtures::label_node_afixture_test(),
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
            assert_eq!(
                meta.reality.server_names,
                xp_test_fixtures::host_list_edge31()
            );
            assert_eq!(endpoint.port, 30445);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[tokio::test]
async fn missing_managed_vless_bootstraps_at_explicit_port() {
    let tempdir = tempfile::tempdir().unwrap();
    let spec = ManagedDefaultEndpointsSpec {
        vless: Some(DefaultVlessEndpointSpec {
            port: 30445,
            reality_dest: "127.0.0.1:39043".to_string(),
            server_names: xp_test_fixtures::host_list_edge30(),
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: "chrome".to_string(),
        }),
        ss: None,
    };
    let mut writes = Vec::<DesiredStateCommand>::new();

    {
        let mut writer = |cmd| {
            writes.push(cmd);
            std::future::ready(Ok(()))
        };
        reconcile_host_managed_default_endpoints(
            tempdir.path(),
            "n1",
            &[],
            HostManagedDefaultEndpointsOptions {
                explicit: &spec,
                access_host: xp_test_fixtures::label_node_afixture_test(),
                vless_canary_bind: "127.0.0.1:39043".parse().unwrap(),
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
            assert_eq!(endpoint.port, 30445);
            assert!(meta.managed_default);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[tokio::test]
async fn existing_managed_ss_preserves_cluster_port_when_bootstrap_port_is_stale() {
    let tempdir = tempfile::tempdir().unwrap();
    let endpoint = endpoint_ss("s1", 30446, Some(true));
    let spec = ManagedDefaultEndpointsSpec {
        vless: None,
        ss: Some(DefaultSsEndpointSpec { port: 30444 }),
    };
    let mut writes = Vec::<DesiredStateCommand>::new();

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
                access_host: xp_test_fixtures::label_node_afixture_test(),
                vless_canary_bind: "127.0.0.1:39043".parse().unwrap(),
            },
            &mut writer,
            "test",
        )
        .await
        .unwrap();
    }

    assert!(writes.is_empty());
}

#[tokio::test]
async fn existing_managed_vless_preserves_cluster_port_when_bootstrap_port_is_stale() {
    let tempdir = tempfile::tempdir().unwrap();
    let endpoint = endpoint_vless("e1", 30445, &["node.example.com"], Some(true));
    let spec = ManagedDefaultEndpointsSpec {
        vless: Some(DefaultVlessEndpointSpec {
            port: 30443,
            reality_dest: "127.0.0.1:39043".to_string(),
            server_names: xp_test_fixtures::host_list_edge30(),
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: "chrome".to_string(),
        }),
        ss: None,
    };
    let mut writes = Vec::<DesiredStateCommand>::new();

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
                access_host: xp_test_fixtures::label_node_afixture_test(),
                vless_canary_bind: "127.0.0.1:39043".parse().unwrap(),
            },
            &mut writer,
            "test",
        )
        .await
        .unwrap();
    }

    let upserted = writes
        .iter()
        .find_map(|command| match command {
            DesiredStateCommand::UpsertEndpoint { endpoint } => Some(endpoint),
            _ => None,
        })
        .expect("managed VLESS reconcile should refresh derived fields");
    assert_eq!(upserted.port, 30445);
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
fn host_managed_existing_explicit_vless_survives_bootstrap_env_removal() {
    let endpoint = endpoint_vless("e1", 30445, &["example.com"], Some(true));
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

    match intent.vless {
        ManagedDefaultEndpointIntent::Preserve { spec, source } => {
            assert_eq!(spec.port, 30445);
            assert_eq!(source, ManagedDefaultEndpointSource::Explicit);
        }
        other => panic!("expected existing endpoint to remain managed, got {other:?}"),
    }
}

#[test]
fn host_managed_existing_explicit_ss_survives_bootstrap_env_removal() {
    let endpoint = endpoint_ss("s1", 30446, Some(true));
    let state = ManagedDefaultEndpointsState {
        schema_version: MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION,
        vless_endpoint_id: None,
        vless_source: None,
        ss_endpoint_id: Some("s1".to_string()),
        ss_source: Some(ManagedDefaultEndpointSource::Explicit),
    };
    let intent = resolve_host_managed_default_endpoints_intent(
        &ManagedDefaultEndpointsSpec::default(),
        &[endpoint],
        "node.example.com",
        "127.0.0.1:39043".parse().unwrap(),
        &state,
    )
    .unwrap();

    match intent.ss {
        ManagedDefaultEndpointIntent::Preserve { spec, source } => {
            assert_eq!(spec.port, 30446);
            assert_eq!(source, ManagedDefaultEndpointSource::Explicit);
        }
        other => panic!("expected existing endpoint to remain managed, got {other:?}"),
    }
}

#[tokio::test]
async fn stale_preserve_intent_does_not_recreate_deleted_vless() {
    let tempdir = tempfile::tempdir().unwrap();
    let endpoint = endpoint_vless("e1", 30445, &["example.com"], Some(true));
    let state = ManagedDefaultEndpointsState {
        schema_version: MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION,
        vless_endpoint_id: Some("e1".to_string()),
        vless_source: Some(ManagedDefaultEndpointSource::Explicit),
        ss_endpoint_id: None,
        ss_source: None,
    };
    persist_managed_default_endpoints_state(tempdir.path(), &state).unwrap();
    let intent = resolve_host_managed_default_endpoints_intent(
        &ManagedDefaultEndpointsSpec::default(),
        &[endpoint],
        "node.example.com",
        "127.0.0.1:39043".parse().unwrap(),
        &state,
    )
    .unwrap();
    let mut writes = Vec::<DesiredStateCommand>::new();

    {
        let mut writer = |cmd| {
            writes.push(cmd);
            std::future::ready(Ok(()))
        };
        reconcile_managed_default_endpoints(
            tempdir.path(),
            "n1",
            &[],
            &intent,
            &mut writer,
            "test",
        )
        .await
        .unwrap();
    }

    assert!(writes.is_empty());
    assert_eq!(
        load_managed_default_endpoints_state(tempdir.path()).unwrap(),
        ManagedDefaultEndpointsState::default()
    );
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
fn host_managed_auto_adopted_vless_keeps_preserve_intent_without_explicit_config() {
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
        ManagedDefaultEndpointIntent::Preserve {
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
            server_names: xp_test_fixtures::host_list_edge31(),
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
    assert_eq!(
        state.vless_endpoint_id.as_deref(),
        Some(xp_test_fixtures::subscription_endpoint_e1())
    );
    assert_eq!(state.ss_endpoint_id, None);
}
