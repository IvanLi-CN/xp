use std::fs;

use pretty_assertions::assert_eq;
use rand::{SeedableRng as _, rngs::StdRng};
use serde_json::json;

use super::*;
use crate::{
    domain::{
        DomainError, Endpoint, EndpointKind, Node, NodeQuotaReset, User, UserPriorityTier,
        UserQuotaReset, validate_cycle_day_of_month, validate_port,
    },
    id::is_ulid_string,
    protocol::{
        RealityConfig, RealityKeys, RealityServerNamesSource, VlessRealityVisionTcpEndpointMeta,
    },
};

mod legacy_smux;

#[derive(Debug, Default)]
struct TestGeoLookup;

impl crate::inbound_ip_usage::GeoLookup for TestGeoLookup {
    fn lookup(&self, _ip: &str) -> crate::inbound_ip_usage::PersistedInboundIpGeo {
        crate::inbound_ip_usage::PersistedInboundIpGeo::default()
    }
}

fn test_init(tmp_dir: &Path) -> StoreInit {
    StoreInit {
        data_dir: tmp_dir.to_path_buf(),
        bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_d().to_owned()),
        bootstrap_node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: xp_test_fixtures::subscription_api_loopback_https().to_owned(),
    }
}

fn test_user(user_id: &str) -> User {
    User {
        user_id: user_id.to_string(),
        display_name: user_id.to_string(),
        subscription_token: xp_test_fixtures::token_fixture512().to_owned(),
        credential_epoch: 0,
        priority_tier: UserPriorityTier::P2,
        quota_reset: UserQuotaReset::default(),
    }
}

fn test_node(_node_id: &str) -> Node {
    Node {
        node_id: xp_test_fixtures::label_node1().to_owned(),
        node_name: xp_test_fixtures::label_node1().to_owned(),
        access_host: xp_test_fixtures::host_fixture513().to_owned(),
        api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    }
}

fn ss_endpoint(endpoint_id: &str, _node_id: &str) -> Endpoint {
    match endpoint_id {
        "endpoint_1" => Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::label_endpoint1().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 10_000,
            meta: json!({}),
        },
        "endpoint_2" => Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint2().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::label_endpoint2().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 10_000,
            meta: json!({}),
        },
        "endpoint_3" => Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint3().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::label_endpoint3().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 10_000,
            meta: json!({}),
        },
        "ss_1" => Endpoint {
            endpoint_id: xp_test_fixtures::label_ss1().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::label_ss1().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 10_000,
            meta: json!({}),
        },
        "ss_2" => Endpoint {
            endpoint_id: xp_test_fixtures::label_ss2().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::label_ss2().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 10_000,
            meta: json!({}),
        },
        _ => panic!("unknown test SS endpoint {endpoint_id}"),
    }
}

fn vless_endpoint(endpoint_id: &str, _node_id: &str) -> Endpoint {
    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: xp_test_fixtures::address_loopback_port39514().to_owned(),
            server_names: xp_test_fixtures::host_list_edge31(),
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

    match endpoint_id {
        "vless_1" => Endpoint {
            endpoint_id: xp_test_fixtures::label_vless1().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::label_vless1().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 443,
            meta: serde_json::to_value(meta).unwrap(),
        },
        "vless_2" => Endpoint {
            endpoint_id: xp_test_fixtures::label_vless2().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::label_vless2().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 443,
            meta: serde_json::to_value(meta).unwrap(),
        },
        _ => panic!("unknown test VLESS endpoint {endpoint_id}"),
    }
}

fn probe_state_with_stale_deleted_node() -> PersistedState {
    let mut state = PersistedState::empty();
    state.nodes.insert(
        "node_keep".to_string(),
        Node {
            node_id: xp_test_fixtures::label_node_keep().to_owned(),
            node_name: xp_test_fixtures::label_keep().to_owned(),
            access_host: xp_test_fixtures::host_fixture516().to_owned(),
            api_base_url: xp_test_fixtures::service_fixture517().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_1".to_string(),
        Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
            node_id: xp_test_fixtures::label_node_keep().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_fixture518().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 443,
            meta: json!({}),
        },
    );
    state.endpoint_probe_participants_by_hour.insert(
        "2026-03-11T11:00:00Z".to_string(),
        BTreeSet::from(["node_keep".to_string(), "node_drop".to_string()]),
    );
    let bucket = state
        .endpoint_probe_history
        .entry("endpoint_1".to_string())
        .or_default()
        .hours
        .entry("2026-03-11T11:00:00Z".to_string())
        .or_default();
    bucket.by_node.insert(
        "node_keep".to_string(),
        EndpointProbeNodeSample {
            ok: true,
            skipped: false,
            checked_at: xp_test_fixtures::timestamp_at20240101_t083900_z().to_owned(),
            latency_ms: xp_test_fixtures::number_value22(),
            target_id: None,
            target_url: None,
            error: None,
            config_hash: xp_test_fixtures::primary_probe_config_hash().to_owned(),
        },
    );
    bucket.by_node.insert(
        "node_drop".to_string(),
        EndpointProbeNodeSample {
            ok: true,
            skipped: false,
            checked_at: xp_test_fixtures::timestamp_at20240101_t084000_z().to_owned(),
            latency_ms: xp_test_fixtures::number_value23(),
            target_id: None,
            target_url: None,
            error: None,
            config_hash: xp_test_fixtures::primary_probe_config_hash().to_owned(),
        },
    );
    state
}

#[test]
fn bootstrap_creates_state_json_with_one_node() {
    let tmp = tempfile::tempdir().unwrap();

    let _store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let state_path = tmp.path().join("state.json");

    assert!(state_path.exists());

    let bytes = fs::read(&state_path).unwrap();
    let state: PersistedState = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(state.schema_version, SCHEMA_VERSION);
    assert_eq!(state.nodes.len(), 1);
    assert_eq!(state.endpoints.len(), 0);
    assert_eq!(state.users.len(), 0);
    assert!(state.node_user_endpoint_memberships.is_empty());

    let (node_id, node) = state.nodes.iter().next().unwrap();
    assert_eq!(node_id, &node.node_id);
    assert_eq!(node.node_name, "node-1");
    assert_eq!(node.access_host, "");
    assert_eq!(node.api_base_url, "https://127.0.0.1:62416");
    assert!(is_ulid_string(&node.node_id));
}

#[test]
fn load_or_init_migrates_v1_state_json_public_domain_to_access_host() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();

    let state_path = data_dir.join("state.json");
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&serde_json::json!({
          "schema_version": 1,
          "nodes": {
            xp_test_fixtures::primary_node_id(): {
              "node_id": xp_test_fixtures::primary_node_id(),
              "node_name": xp_test_fixtures::primary_node_name(),
              "public_domain": xp_test_fixtures::primary_host(),
              "api_base_url": xp_test_fixtures::primary_api_url()
            }
          }
        }))
        .unwrap(),
    )
    .unwrap();

    let store = JsonSnapshotStore::load_or_init(StoreInit {
        data_dir: data_dir.to_path_buf(),
        bootstrap_node_id: None,
        bootstrap_node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: xp_test_fixtures::subscription_api_loopback_https().to_owned(),
    })
    .unwrap();
    assert_eq!(store.state().schema_version, SCHEMA_VERSION);
    let nodes = &store.state().nodes;
    let node = nodes.get(xp_test_fixtures::primary_node_id()).unwrap();
    assert_eq!(node.access_host, xp_test_fixtures::primary_host());
    let bytes = fs::read(&state_path).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved["schema_version"], SCHEMA_VERSION);
    let saved_node = &saved["nodes"][xp_test_fixtures::primary_node_id()];
    assert!(saved_node.get("access_host").is_some());
    assert!(saved_node.get("public_domain").is_none());
}

#[test]
fn load_or_init_persists_pruned_usage_memberships() {
    let tmp = tempfile::tempdir().unwrap();
    let valid_membership_key = {
        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        let node_id = store.list_nodes()[0].node_id.clone();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                31234,
                json!({}),
            )
            .unwrap();
        let membership = membership_key(&user.user_id, &endpoint.endpoint_id);

        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id,
            endpoint_ids: vec![endpoint.endpoint_id],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        store
            .apply_membership_usage_sample(
                &membership,
                "2026-01-01T00:00:00Z".to_string(),
                "2026-02-01T00:00:00Z".to_string(),
                1,
                0,
                "2026-01-01T00:00:01Z".to_string(),
            )
            .unwrap();
        store.usage.memberships.insert(
            "stale_user::stale_endpoint".to_string(),
            MembershipUsage {
                cycle_start_at: xp_test_fixtures::baseline_timestamp().to_owned(),
                cycle_end_at: xp_test_fixtures::recent_timestamp().to_owned(),
                used_bytes: 10,
                last_uplink_total: 10,
                last_downlink_total: 0,
                last_seen_at: xp_test_fixtures::timestamp_at20240101_t084100_z().to_owned(),
                quota_banned: false,
                quota_banned_at: None,
            },
        );
        store.save_usage().unwrap();
        membership
    };

    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    assert!(
        store
            .get_membership_usage("stale_user::stale_endpoint")
            .is_none()
    );
    assert!(store.get_membership_usage(&valid_membership_key).is_some());

    let usage_path = tmp.path().join("usage.json");
    let bytes = fs::read(usage_path).unwrap();
    let usage: PersistedUsage = serde_json::from_slice(&bytes).unwrap();
    assert!(!usage.memberships.contains_key("stale_user::stale_endpoint"));
    assert!(usage.memberships.contains_key(&valid_membership_key));
}

#[test]
fn load_or_init_recovers_when_state_is_v10_but_usage_is_v1() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();

    // Simulate an interrupted upgrade: state.json is already v10 (no grants), but usage.json
    // is still the legacy v1 grants map (cannot be migrated without a grant mapping).
    let node_id = "node_1".to_string();
    let endpoint_id = "endpoint_1".to_string();
    let user_id = "user_1".to_string();

    let mut state = PersistedState::empty();
    state.nodes.insert(
        node_id.clone(),
        Node {
            node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
            node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
            access_host: xp_test_fixtures::label_empty().to_owned(),
            api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        endpoint_id.clone(),
        Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
            node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
            tag: xp_test_fixtures::label_endpoint2().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 31234,
            meta: json!({}),
        },
    );
    state.users.insert(
        user_id.clone(),
        User {
            user_id: user_id.clone(),
            display_name: "alice".to_string(),
            subscription_token: xp_test_fixtures::label_sub1().to_owned(),
            credential_epoch: 0,
            priority_tier: UserPriorityTier::P2,
            quota_reset: UserQuotaReset::default(),
        },
    );
    state
        .node_user_endpoint_memberships
        .insert(NodeUserEndpointMembership {
            user_id: user_id.clone(),
            node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
            endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
        });

    let state_path = data_dir.join("state.json");
    fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let usage_path = data_dir.join("usage.json");
    fs::write(
        &usage_path,
        serde_json::to_vec_pretty(&json!({
          "schema_version": 1,
          "grants": {
            "grant_1": {
              "cycle_start_at": "2026-01-01T00:00:00Z",
              "cycle_end_at": "2026-02-01T00:00:00Z",
              "used_bytes": 123,
              "last_uplink_total": 123,
              "last_downlink_total": 0,
              "last_seen_at": "2026-01-01T00:00:01Z",
              "quota_banned": false,
              "quota_banned_at": null
            }
          }
        }))
        .unwrap(),
    )
    .unwrap();

    let store = JsonSnapshotStore::load_or_init(test_init(data_dir)).unwrap();
    assert_eq!(store.state().schema_version, SCHEMA_VERSION);
    assert_eq!(store.usage.schema_version, USAGE_SCHEMA_VERSION);
    assert!(store.usage.memberships.is_empty());

    let bytes = fs::read(&usage_path).unwrap();
    let saved: PersistedUsage = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved.schema_version, USAGE_SCHEMA_VERSION);
}

#[test]
fn legacy_set_grant_enabled_missing_source_deserializes_as_noop() {
    let cmd: DesiredStateCommand = serde_json::from_value(json!({
        "type": "set_grant_enabled",
        "grant_id": "grant_1",
        "enabled": false
    }))
    .unwrap();

    match cmd {
        DesiredStateCommand::CompatNoop { note } => {
            assert!(note.contains("legacy set_grant_enabled ignored"))
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn legacy_replace_user_access_items_deserializes_to_endpoint_ids() {
    let cmd: DesiredStateCommand = serde_json::from_value(json!({
        "type": "replace_user_access",
        "user_id": "user_1",
        "items": [
            { "endpoint_id": "endpoint_2", "note": "legacy note" },
            { "endpoint_id": "endpoint_1" }
        ]
    }))
    .unwrap();

    match cmd {
        DesiredStateCommand::ReplaceUserAccess {
            user_id,
            endpoint_ids,
        } => {
            assert_eq!(user_id, "user_1");
            // Compat mapping is allowed to sort/dedup.
            assert_eq!(endpoint_ids, vec!["endpoint_1", "endpoint_2"]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn compat_noop_can_carry_node_egress_probe_state() {
    let mut state = PersistedState::empty();
    state.nodes.insert(
        "node-1".to_string(),
        Node {
            node_id: xp_test_fixtures::node_id_fixture523().to_owned(),
            node_name: xp_test_fixtures::label_tokyo().to_owned(),
            access_host: xp_test_fixtures::host_fixture524().to_owned(),
            api_base_url: xp_test_fixtures::service_fixture525().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    let probe = NodeEgressProbeState {
        selected_public_ip: Some(xp_test_fixtures::address_documentation192_0_2_127().to_owned()),
        subscription_region: NodeSubscriptionRegion::Taiwan,
        checked_at: xp_test_fixtures::timestamp_at20240101_t080800_z().to_owned(),
        last_success_at: Some(xp_test_fixtures::timestamp_at20260424_t000000_z().to_owned()),
        ..NodeEgressProbeState::default()
    };
    let note = encode_node_egress_probe_compat_note("node-1", &probe).unwrap();

    let result = DesiredStateCommand::CompatNoop { note }
        .apply(&mut state)
        .unwrap();

    assert_eq!(result, DesiredStateApplyResult::Applied);
    assert_eq!(state.node_egress_probes.get("node-1"), Some(&probe));
}

#[test]
fn user_mihomo_profile_serializes_and_deserializes_mixin_yaml() {
    let profile: UserMihomoProfile = serde_json::from_value(json!({
        "mixin_yaml": "port: 0
rules: []
",
        "extra_proxies_yaml": "",
        "extra_proxy_providers_yaml": ""
    }))
    .unwrap();

    assert_eq!(
        profile.mixin_yaml,
        "port: 0
rules: []
"
    );

    let serialized = serde_json::to_value(&profile).unwrap();
    assert_eq!(
        serialized["mixin_yaml"],
        "port: 0
rules: []
"
    );
    assert_eq!(
        serialized["template_yaml"],
        "port: 0
rules: []
"
    );
}

#[test]
fn user_mihomo_profile_deserializes_legacy_template_yaml_for_internal_compat() {
    let profile: UserMihomoProfile = serde_json::from_value(json!({
        "template_yaml": "port: 0
rules: []
",
        "extra_proxies_yaml": "",
        "extra_proxy_providers_yaml": ""
    }))
    .unwrap();

    assert_eq!(
        profile.mixin_yaml,
        "port: 0
rules: []
"
    );

    let serialized = serde_json::to_value(&profile).unwrap();
    assert_eq!(
        serialized["mixin_yaml"],
        "port: 0
rules: []
"
    );
    assert_eq!(
        serialized["template_yaml"],
        "port: 0
rules: []
"
    );
}

#[test]
fn desired_state_command_set_user_mihomo_profile_serializes_internal_compat_fields() {
    let serialized = serde_json::to_value(DesiredStateCommand::SetUserMihomoProfile {
        user_id: "user_1".to_string(),
        profile: UserMihomoProfile {
            mixin_yaml: "port: 0
rules: []
"
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        },
    })
    .unwrap();

    let profile = &serialized["profile"];
    assert_eq!(
        profile["mixin_yaml"],
        "port: 0
rules: []
"
    );
    assert_eq!(
        profile["template_yaml"],
        "port: 0
rules: []
"
    );
}

#[test]
fn user_mihomo_profile_deserializes_dual_written_internal_payload() {
    let profile: UserMihomoProfile = serde_json::from_value(json!({
        "mixin_yaml": "port: 1
rules: []
",
        "template_yaml": "port: 0
rules: []
",
        "extra_proxies_yaml": "",
        "extra_proxy_providers_yaml": ""
    }))
    .unwrap();

    assert_eq!(
        profile.mixin_yaml,
        "port: 1
rules: []
"
    );
}

#[test]
fn desired_state_command_serialization_keeps_template_yaml_for_legacy_nodes() {
    #[derive(Debug, Deserialize)]
    struct LegacyUserMihomoProfileCompat {
        #[serde(default)]
        template_yaml: String,
        #[serde(default)]
        extra_proxies_yaml: String,
        #[serde(default)]
        extra_proxy_providers_yaml: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum LegacyDesiredStateCommandCompat {
        SetUserMihomoProfile {
            user_id: String,
            profile: LegacyUserMihomoProfileCompat,
        },
    }

    let serialized = serde_json::to_value(DesiredStateCommand::SetUserMihomoProfile {
        user_id: "user_1".to_string(),
        profile: UserMihomoProfile {
            mixin_yaml: "port: 0
rules: []
"
            .to_string(),
            extra_proxies_yaml: "".to_string(),
            extra_proxy_providers_yaml: "".to_string(),
        },
    })
    .unwrap();

    let legacy: LegacyDesiredStateCommandCompat = serde_json::from_value(serialized).unwrap();
    match legacy {
        LegacyDesiredStateCommandCompat::SetUserMihomoProfile { user_id, profile } => {
            assert_eq!(user_id, "user_1");
            assert_eq!(
                profile.template_yaml,
                "port: 0
rules: []
"
            );
            assert_eq!(profile.extra_proxies_yaml, "");
            assert_eq!(profile.extra_proxy_providers_yaml, "");
        }
    }
}

#[test]
fn desired_state_command_deserializes_legacy_template_yaml_profile_for_internal_compat() {
    let cmd: DesiredStateCommand = serde_json::from_value(json!({
        "type": "set_user_mihomo_profile",
        "user_id": "user_1",
        "profile": {
            "template_yaml": "port: 0
rules: []
",
            "extra_proxies_yaml": "",
            "extra_proxy_providers_yaml": ""
        }
    }))
    .unwrap();

    match cmd {
        DesiredStateCommand::SetUserMihomoProfile { user_id, profile } => {
            assert_eq!(user_id, "user_1");
            assert_eq!(
                profile.mixin_yaml,
                "port: 0
rules: []
"
            );
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn migrate_state_value_to_latest_accepts_v10_template_yaml_mihomo_profiles() {
    let mut raw = serde_json::to_value(PersistedState::empty()).unwrap();
    raw["users"] = json!({
        "user_1": {
            "user_id": "user_1",
            "display_name": "alice",
            "subscription_token": "sub_1",
            "credential_epoch": 0,
            "priority_tier": "p2",
            "quota_reset": {
                "policy": "monthly",
                "day_of_month": 1,
                "tz_offset_minutes": 480
            }
        }
    });
    raw["user_mihomo_profiles"] = json!({
        "user_1": {
            "template_yaml": "port: 0
rules: []
",
            "extra_proxies_yaml": "",
            "extra_proxy_providers_yaml": ""
        }
    });

    let state = migrate_state_value_to_latest(raw).expect("legacy v10 state should load");
    let profile = state
        .user_mihomo_profiles
        .get("user_1")
        .expect("profile should exist after migration");
    assert_eq!(
        profile.mixin_yaml,
        "port: 0
rules: []
"
    );
}

#[test]
fn migrate_state_value_to_latest_prunes_deleted_probe_nodes_from_current_schema_state() {
    let raw = serde_json::to_value(probe_state_with_stale_deleted_node()).unwrap();

    let state = migrate_state_value_to_latest(raw).expect("current-schema state should load");

    assert_eq!(
        state
            .endpoint_probe_participants_by_hour
            .get("2026-03-11T11:00:00Z"),
        Some(&BTreeSet::from(["node_keep".to_string()])),
    );
    let bucket = state
        .endpoint_probe_history
        .get("endpoint_1")
        .and_then(|history| history.hours.get("2026-03-11T11:00:00Z"))
        .expect("endpoint probe bucket should survive for the kept node");
    assert_eq!(
        bucket.by_node.keys().cloned().collect::<Vec<_>>(),
        vec!["node_keep".to_string()],
    );
}

#[test]
fn replace_user_access_reports_delta_counts_not_physical_rewrites() {
    let mut state = PersistedState::empty();
    state
        .users
        .insert("user_1".to_string(), test_user("user_1"));
    state
        .nodes
        .insert("node_1".to_string(), test_node("node_1"));
    for endpoint_id in ["endpoint_1", "endpoint_2", "endpoint_3"] {
        state
            .endpoints
            .insert(endpoint_id.to_string(), ss_endpoint(endpoint_id, "node_1"));
    }

    // Seed initial access: endpoint_1 + endpoint_2.
    DesiredStateCommand::ReplaceUserAccess {
        user_id: "user_1".to_string(),
        endpoint_ids: vec!["endpoint_1".to_string(), "endpoint_2".to_string()],
    }
    .apply(&mut state)
    .unwrap();

    // Replace: drop endpoint_1, keep endpoint_2, add endpoint_3.
    let out = DesiredStateCommand::ReplaceUserAccess {
        user_id: "user_1".to_string(),
        endpoint_ids: vec!["endpoint_2".to_string(), "endpoint_3".to_string()],
    }
    .apply(&mut state)
    .unwrap();

    assert!(
        matches!(
            out,
            DesiredStateApplyResult::UserAccessReplaced {
                created: 1,
                deleted: 1
            }
        ),
        "unexpected apply result: {out:?}"
    );

    let endpoints = state
        .node_user_endpoint_memberships
        .iter()
        .filter(|m| m.user_id == "user_1")
        .map(|m| m.endpoint_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(endpoints, BTreeSet::from(["endpoint_2", "endpoint_3"]));
}

#[test]
fn replace_user_access_records_all_selected_endpoint_kinds() {
    let mut state = PersistedState::empty();
    state
        .users
        .insert("user_1".to_string(), test_user("user_1"));
    state
        .nodes
        .insert("node_1".to_string(), test_node("node_1"));
    state
        .endpoints
        .insert("ss_1".to_string(), ss_endpoint("ss_1", "node_1"));
    state
        .endpoints
        .insert("ss_2".to_string(), ss_endpoint("ss_2", "node_1"));
    state
        .endpoints
        .insert("vless_1".to_string(), vless_endpoint("vless_1", "node_1"));

    DesiredStateCommand::ReplaceUserAccess {
        user_id: "user_1".to_string(),
        endpoint_ids: vec!["ss_1".to_string(), "ss_2".to_string()],
    }
    .apply(&mut state)
    .unwrap();

    assert_eq!(
        state.user_auto_assign_endpoint_kinds.get("user_1"),
        Some(&BTreeSet::from([EndpointKind::Ss2022_2022Blake3Aes128Gcm]))
    );
}

#[test]
fn replace_user_access_clears_auto_kind_when_subset_selected() {
    let mut state = PersistedState::empty();
    state
        .users
        .insert("user_1".to_string(), test_user("user_1"));
    state
        .nodes
        .insert("node_1".to_string(), test_node("node_1"));
    state
        .endpoints
        .insert("ss_1".to_string(), ss_endpoint("ss_1", "node_1"));
    state
        .endpoints
        .insert("ss_2".to_string(), ss_endpoint("ss_2", "node_1"));

    DesiredStateCommand::ReplaceUserAccess {
        user_id: "user_1".to_string(),
        endpoint_ids: vec!["ss_1".to_string(), "ss_2".to_string()],
    }
    .apply(&mut state)
    .unwrap();
    assert!(state.user_auto_assign_endpoint_kinds.contains_key("user_1"));

    DesiredStateCommand::ReplaceUserAccess {
        user_id: "user_1".to_string(),
        endpoint_ids: vec!["ss_1".to_string()],
    }
    .apply(&mut state)
    .unwrap();

    assert!(!state.user_auto_assign_endpoint_kinds.contains_key("user_1"));
    let endpoints = state
        .node_user_endpoint_memberships
        .iter()
        .filter(|m| m.user_id == "user_1")
        .map(|m| m.endpoint_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(endpoints, BTreeSet::from(["ss_1"]));
}

#[test]
fn upsert_endpoint_auto_grants_matching_kind_only() {
    let mut state = PersistedState::empty();
    for user_id in ["vless_user", "ss_user"] {
        state.users.insert(user_id.to_string(), test_user(user_id));
    }
    state
        .nodes
        .insert("node_1".to_string(), test_node("node_1"));
    state
        .endpoints
        .insert("vless_1".to_string(), vless_endpoint("vless_1", "node_1"));
    state
        .endpoints
        .insert("ss_1".to_string(), ss_endpoint("ss_1", "node_1"));

    DesiredStateCommand::ReplaceUserAccess {
        user_id: "vless_user".to_string(),
        endpoint_ids: vec!["vless_1".to_string()],
    }
    .apply(&mut state)
    .unwrap();
    DesiredStateCommand::ReplaceUserAccess {
        user_id: "ss_user".to_string(),
        endpoint_ids: vec!["ss_1".to_string()],
    }
    .apply(&mut state)
    .unwrap();

    DesiredStateCommand::UpsertEndpoint {
        endpoint: vless_endpoint("vless_2", "node_1"),
        expected: None,
    }
    .apply(&mut state)
    .unwrap();

    let endpoints_by_user = |state: &PersistedState, user_id: &str| {
        state
            .node_user_endpoint_memberships
            .iter()
            .filter(|m| m.user_id == user_id)
            .map(|m| m.endpoint_id.clone())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(
        endpoints_by_user(&state, "vless_user"),
        BTreeSet::from(["vless_1".to_string(), "vless_2".to_string()])
    );
    assert_eq!(
        endpoints_by_user(&state, "ss_user"),
        BTreeSet::from(["ss_1".to_string()])
    );
}

#[test]
fn delete_last_endpoint_preserves_auto_kind_for_future_endpoint() {
    let mut state = PersistedState::empty();
    state
        .users
        .insert("user_1".to_string(), test_user("user_1"));
    state
        .nodes
        .insert("node_1".to_string(), test_node("node_1"));
    state
        .endpoints
        .insert("ss_1".to_string(), ss_endpoint("ss_1", "node_1"));

    DesiredStateCommand::ReplaceUserAccess {
        user_id: "user_1".to_string(),
        endpoint_ids: vec!["ss_1".to_string()],
    }
    .apply(&mut state)
    .unwrap();

    DesiredStateCommand::DeleteEndpoint {
        endpoint_id: xp_test_fixtures::label_ss1().to_owned(),
    }
    .apply(&mut state)
    .unwrap();
    assert_eq!(
        state.user_auto_assign_endpoint_kinds.get("user_1"),
        Some(&BTreeSet::from([EndpointKind::Ss2022_2022Blake3Aes128Gcm]))
    );
    assert!(state.node_user_endpoint_memberships.is_empty());

    DesiredStateCommand::UpsertEndpoint {
        endpoint: ss_endpoint("ss_2", "node_1"),
        expected: None,
    }
    .apply(&mut state)
    .unwrap();

    let endpoints = state
        .node_user_endpoint_memberships
        .iter()
        .filter(|m| m.user_id == "user_1")
        .map(|m| m.endpoint_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(endpoints, BTreeSet::from(["ss_2"]));
}

#[test]
fn upsert_vless_endpoint_manual_preserves_dest() {
    let mut state = PersistedState::empty();

    let endpoint_id = xp_test_fixtures::label_endpoint1().to_owned();

    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: xp_test_fixtures::address_loopback_port39528().to_owned(),
            server_names: xp_test_fixtures::host_list_edge_bfixture_test(),
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

    DesiredStateCommand::UpsertEndpoint {
        endpoint,
        expected: None,
    }
    .apply(&mut state)
    .unwrap();

    let saved = state.endpoints.get(&endpoint_id).unwrap();
    let meta: VlessRealityVisionTcpEndpointMeta =
        serde_json::from_value(saved.meta.clone()).expect("vless meta");

    assert_eq!(
        meta.reality.server_names,
        vec![
            "edge-b.fixture.test".to_string(),
            "edge-a.fixture.test".to_string(),
        ]
    );
    assert_eq!(
        meta.reality.dest,
        xp_test_fixtures::address_loopback_port39528()
    );
}

#[test]
fn upsert_vless_endpoint_manual_rejects_invalid_dest() {
    let mut state = PersistedState::empty();

    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: xp_test_fixtures::label_empty().to_owned(),
            server_names: xp_test_fixtures::host_list_edge31(),
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

    let err = DesiredStateCommand::UpsertEndpoint {
        endpoint,
        expected: None,
    }
    .apply(&mut state)
    .unwrap_err();
    assert!(err.to_string().contains("dest is required"));
    assert!(state.endpoints.is_empty());
}

#[test]
fn upsert_vless_endpoint_global_derives_server_names_and_dest() {
    let mut state = PersistedState::empty();

    state.reality_domains = vec![
        crate::domain::RealityDomain {
            domain_id: "d1".to_string(),
            server_name: "first.example.com".to_string(),
            disabled_node_ids: BTreeSet::new(),
        },
        crate::domain::RealityDomain {
            domain_id: "d2".to_string(),
            server_name: "second.example.com".to_string(),
            disabled_node_ids: BTreeSet::from(["node_1".to_string()]),
        },
        crate::domain::RealityDomain {
            domain_id: "d3".to_string(),
            server_name: "third.example.com".to_string(),
            disabled_node_ids: BTreeSet::new(),
        },
    ];

    let endpoint_id = xp_test_fixtures::label_endpoint1().to_owned();

    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: xp_test_fixtures::label_empty().to_owned(),
            server_names: xp_test_fixtures::host_list_empty(),
            server_names_source: RealityServerNamesSource::Global,
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

    DesiredStateCommand::UpsertEndpoint {
        endpoint,
        expected: None,
    }
    .apply(&mut state)
    .unwrap();

    let saved = state.endpoints.get(&endpoint_id).unwrap();
    let meta: VlessRealityVisionTcpEndpointMeta =
        serde_json::from_value(saved.meta.clone()).expect("vless meta");

    assert_eq!(
        meta.reality.server_names,
        vec![
            "first.example.com".to_string(),
            "third.example.com".to_string()
        ]
    );
    assert_eq!(meta.reality.dest, "first.example.com:443");
}

#[test]
fn upsert_managed_default_vless_global_preserves_canary_dest() {
    let mut state = PersistedState::empty();

    state.reality_domains = vec![
        crate::domain::RealityDomain {
            domain_id: "d1".to_string(),
            server_name: "first.example.com".to_string(),
            disabled_node_ids: BTreeSet::new(),
        },
        crate::domain::RealityDomain {
            domain_id: "d2".to_string(),
            server_name: "third.example.com".to_string(),
            disabled_node_ids: BTreeSet::new(),
        },
    ];

    let endpoint_id = xp_test_fixtures::label_endpoint1().to_owned();

    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: xp_test_fixtures::address_loopback_port39531().to_owned(),
            server_names: xp_test_fixtures::host_list_empty(),
            server_names_source: RealityServerNamesSource::Global,
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
        managed_default: true,
    };

    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
        node_id: xp_test_fixtures::label_node1().to_owned(),
        tag: xp_test_fixtures::label_vless_test().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::to_value(meta).unwrap(),
    };

    DesiredStateCommand::UpsertEndpoint {
        endpoint,
        expected: None,
    }
    .apply(&mut state)
    .unwrap();

    let saved = state.endpoints.get(&endpoint_id).unwrap();
    let meta: VlessRealityVisionTcpEndpointMeta =
        serde_json::from_value(saved.meta.clone()).expect("vless meta");

    assert_eq!(
        meta.reality.server_names,
        vec![
            "first.example.com".to_string(),
            "third.example.com".to_string()
        ]
    );
    assert_eq!(
        meta.reality.dest,
        xp_test_fixtures::address_loopback_port39531()
    );
}

#[test]
fn save_load_roundtrip_persists_entities() {
    let tmp = tempfile::tempdir().unwrap();

    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let user = store.create_user("alice".to_string(), None).unwrap();

    drop(store);

    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    assert!(store.state().users.contains_key(&user.user_id));
}

#[test]
fn validation_rejects_invalid_cycle_day_of_month() {
    assert!(validate_cycle_day_of_month(0).is_err());
    assert!(validate_cycle_day_of_month(32).is_err());
    assert!(validate_cycle_day_of_month(1).is_ok());
    assert!(validate_cycle_day_of_month(31).is_ok());
}

#[test]
fn validation_rejects_invalid_port() {
    assert!(validate_port(0).is_err());
    assert!(validate_port(1).is_ok());
    assert!(validate_port(65535).is_ok());
}

#[test]
fn load_usage_json_missing_quota_fields_is_backward_compatible() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path()).unwrap();

    let membership = {
        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                store.list_nodes()[0].node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                443,
                json!({}),
            )
            .unwrap();
        let membership = membership_key(&user.user_id, &endpoint.endpoint_id);
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id,
            endpoint_ids: vec![endpoint.endpoint_id],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        membership
    };
    let usage_path = tmp.path().join("usage.json");
    let bytes = serde_json::to_vec_pretty(&json!({
        "schema_version": USAGE_SCHEMA_VERSION,
        "memberships": {
            membership.clone(): {
                "cycle_start_at": "2025-12-01T00:00:00Z",
                "cycle_end_at": "2026-01-01T00:00:00Z",
                "used_bytes": 123,
                "last_uplink_total": 100,
                "last_downlink_total": 23,
                "last_seen_at": "2025-12-18T00:00:00Z"
            }
        }
    }))
    .unwrap();
    fs::write(&usage_path, bytes).unwrap();

    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let usage = store.get_membership_usage(&membership).unwrap();
    assert!(!usage.quota_banned);
    assert_eq!(usage.quota_banned_at, None);
}

#[test]
fn set_and_clear_quota_banned_persists_and_survives_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let banned_at = "2025-12-18T00:00:00Z".to_string();
    let membership = {
        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                store.list_nodes()[0].node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                443,
                json!({}),
            )
            .unwrap();
        let membership = membership_key(&user.user_id, &endpoint.endpoint_id);
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id,
            endpoint_ids: vec![endpoint.endpoint_id],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        membership
    };

    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    store
        .set_quota_banned(&membership, banned_at.clone())
        .unwrap();
    let usage = store.get_membership_usage(&membership).unwrap();
    assert!(usage.quota_banned);
    assert_eq!(usage.quota_banned_at, Some(banned_at.clone()));

    store.clear_quota_banned(&membership).unwrap();
    let usage = store.get_membership_usage(&membership).unwrap();
    assert!(!usage.quota_banned);
    assert_eq!(usage.quota_banned_at, None);

    drop(store);

    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let usage = store.get_membership_usage(&membership).unwrap();
    assert!(!usage.quota_banned);
    assert_eq!(usage.quota_banned_at, None);
}

#[test]
fn apply_membership_usage_sample_keeps_quota_markers_on_cycle_change() {
    let tmp = tempfile::tempdir().unwrap();
    let membership_key = "user_1::endpoint_1";
    let banned_at = "2025-12-18T00:00:00Z".to_string();

    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    store
        .apply_membership_usage_sample(
            membership_key,
            "2025-12-01T00:00:00Z".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
            10,
            20,
            "2025-12-18T00:00:00Z".to_string(),
        )
        .unwrap();
    store
        .set_quota_banned(membership_key, banned_at.clone())
        .unwrap();

    store
        .apply_membership_usage_sample(
            membership_key,
            "2026-01-01T00:00:00Z".to_string(),
            "2026-02-01T00:00:00Z".to_string(),
            0,
            0,
            "2026-01-01T00:00:00Z".to_string(),
        )
        .unwrap();

    let usage = store.get_membership_usage(membership_key).unwrap();
    assert!(usage.quota_banned);
    assert_eq!(usage.quota_banned_at, Some(banned_at));
}

#[test]
fn clear_membership_usage_removes_usage_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let membership_key = "user_1::endpoint_1";

    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    store
        .set_quota_banned(membership_key, "2025-12-18T00:00:00Z".to_string())
        .unwrap();
    assert!(store.get_membership_usage(membership_key).is_some());

    store.clear_membership_usage(membership_key).unwrap();
    assert!(store.get_membership_usage(membership_key).is_none());

    drop(store);

    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    assert!(store.get_membership_usage(membership_key).is_none());
}

#[test]
fn record_inbound_ip_usage_samples_persists_minute_and_warning_state() {
    let tmp = tempfile::tempdir().unwrap();
    let (membership_key, minute) = {
        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        let node_id = store.list_nodes()[0].node_id.clone();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                443,
                json!({}),
            )
            .unwrap();
        let membership_key = membership_key(&user.user_id, &endpoint.endpoint_id);
        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();
        let minute = crate::inbound_ip_usage::floor_minute(chrono::Utc::now());
        let resolver = TestGeoLookup;
        store
            .record_inbound_ip_usage_samples(
                minute,
                true,
                &[crate::inbound_ip_usage::InboundIpMinuteSample {
                    membership_key: membership_key.clone(),
                    user_id: user.user_id,
                    node_id,
                    endpoint_id: xp_test_fixtures::endpoint_id_fixture533().to_owned(),
                    endpoint_tag: xp_test_fixtures::endpoint_tag_fixture534().to_owned(),
                    ips: vec!["203.0.113.7".to_string()],
                }],
                &resolver,
                true,
            )
            .unwrap();
        (membership_key, minute.to_rfc3339())
    };

    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let inbound = store.inbound_ip_usage();
    assert_eq!(inbound.latest_minute.as_deref(), Some(minute.as_str()));
    assert!(inbound.online_stats_unavailable);
    assert_eq!(
        inbound.memberships[&membership_key].ips["203.0.113.7"].minutes,
        1
    );
}

#[test]
fn prune_and_clear_inbound_ip_usage_remove_stale_memberships() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let node_id = store.list_nodes()[0].node_id.clone();
    let user = store.create_user("alice".to_string(), None).unwrap();
    let endpoint = store
        .create_endpoint(
            node_id.clone(),
            EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            443,
            json!({}),
        )
        .unwrap();
    let valid_membership_key = membership_key(&user.user_id, &endpoint.endpoint_id);
    DesiredStateCommand::ReplaceUserAccess {
        user_id: user.user_id.clone(),
        endpoint_ids: vec![endpoint.endpoint_id.clone()],
    }
    .apply(store.state_mut())
    .unwrap();
    store.save().unwrap();

    let minute = crate::inbound_ip_usage::floor_minute(chrono::Utc::now());
    let resolver = TestGeoLookup;
    store
        .record_inbound_ip_usage_samples(
            minute,
            false,
            &[
                crate::inbound_ip_usage::InboundIpMinuteSample {
                    membership_key: valid_membership_key.clone(),
                    user_id: user.user_id,
                    node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
                    endpoint_id: xp_test_fixtures::endpoint_id_fixture456().to_owned(),
                    endpoint_tag: xp_test_fixtures::endpoint_tag_fixture535().to_owned(),
                    ips: vec!["203.0.113.7".to_string()],
                },
                crate::inbound_ip_usage::InboundIpMinuteSample {
                    membership_key: "stale-user::stale-endpoint".to_string(),
                    user_id: "stale-user".to_string(),
                    node_id,
                    endpoint_id: xp_test_fixtures::label_vless2().to_owned(),
                    endpoint_tag: xp_test_fixtures::label_vless2().to_owned(),
                    ips: vec!["198.51.100.9".to_string()],
                },
            ],
            &resolver,
            true,
        )
        .unwrap();

    assert!(
        store
            .inbound_ip_usage()
            .memberships
            .contains_key("stale-user::stale-endpoint")
    );

    store.prune_inbound_ip_usage_memberships().unwrap();
    assert!(
        !store
            .inbound_ip_usage()
            .memberships
            .contains_key("stale-user::stale-endpoint")
    );
    assert!(
        store
            .inbound_ip_usage()
            .memberships
            .contains_key(&valid_membership_key)
    );

    store
        .clear_membership_inbound_ip_usage(&valid_membership_key)
        .unwrap();
    assert!(
        !store
            .inbound_ip_usage()
            .memberships
            .contains_key(&valid_membership_key)
    );

    drop(store);
    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    assert!(
        !store
            .inbound_ip_usage()
            .memberships
            .contains_key("stale-user::stale-endpoint")
    );
    assert!(
        !store
            .inbound_ip_usage()
            .memberships
            .contains_key(&valid_membership_key)
    );
}

#[test]
fn desired_state_apply_upsert_node_inserts_node() {
    let mut state = PersistedState::empty();
    let node = Node {
        node_id: xp_test_fixtures::label_node1().to_owned(),
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::host_fixture465().to_owned(),
        api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };

    DesiredStateCommand::UpsertNode { node: node.clone() }
        .apply(&mut state)
        .unwrap();

    assert_eq!(state.nodes.get(&node.node_id), Some(&node));
}

#[test]
fn desired_state_apply_endpoint_create_and_delete_are_deterministic() {
    let mut state = PersistedState::empty();
    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::endpoint_id_fixture538().to_owned(),
        node_id: xp_test_fixtures::label_node1().to_owned(),
        tag: xp_test_fixtures::endpoint_tag_fixture539().to_owned(),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port: 443,
        meta: json!({"k":"v"}),
    };

    DesiredStateCommand::UpsertEndpoint {
        endpoint: endpoint.clone(),
        expected: None,
    }
    .apply(&mut state)
    .unwrap();
    assert_eq!(state.endpoints.get(&endpoint.endpoint_id), Some(&endpoint));

    let out = DesiredStateCommand::DeleteEndpoint {
        endpoint_id: xp_test_fixtures::endpoint_id_fixture538().to_owned(),
    }
    .apply(&mut state)
    .unwrap();
    assert_eq!(
        out,
        DesiredStateApplyResult::EndpointDeleted { deleted: true }
    );
    assert!(!state.endpoints.contains_key(&endpoint.endpoint_id));
}

#[test]
fn desired_state_apply_rejects_invalid_port() {
    let mut state = PersistedState::empty();
    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::endpoint_id_fixture538().to_owned(),
        node_id: xp_test_fixtures::label_node1().to_owned(),
        tag: xp_test_fixtures::endpoint_tag_fixture539().to_owned(),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port: 0,
        meta: json!({}),
    };

    let err = DesiredStateCommand::UpsertEndpoint {
        endpoint,
        expected: None,
    }
    .apply(&mut state)
    .unwrap_err();

    assert!(matches!(
        err,
        StoreError::Domain(DomainError::InvalidPort { .. })
    ));
}

#[test]
fn desired_state_apply_user_create_reset_token_and_delete_are_deterministic() {
    let mut state = PersistedState::empty();
    let user = User {
        user_id: "user_1".to_string(),
        display_name: "alice".to_string(),
        subscription_token: xp_test_fixtures::label_sub1().to_owned(),
        credential_epoch: 0,
        priority_tier: Default::default(),
        quota_reset: UserQuotaReset::default(),
    };

    DesiredStateCommand::UpsertUser { user: user.clone() }
        .apply(&mut state)
        .unwrap();
    assert_eq!(state.users.get(&user.user_id), Some(&user));

    let out = DesiredStateCommand::ResetUserSubscriptionToken {
        user_id: user.user_id.clone(),
        subscription_token: xp_test_fixtures::label_sub2().to_owned(),
    }
    .apply(&mut state)
    .unwrap();
    assert_eq!(
        out,
        DesiredStateApplyResult::UserTokenReset { applied: true }
    );
    assert_eq!(
        state
            .users
            .get(&user.user_id)
            .unwrap()
            .subscription_token
            .as_str(),
        "sub_2"
    );

    let out = DesiredStateCommand::DeleteUser {
        user_id: user.user_id.clone(),
    }
    .apply(&mut state)
    .unwrap();
    assert_eq!(out, DesiredStateApplyResult::UserDeleted { deleted: true });
    assert!(!state.users.contains_key(&user.user_id));
}

#[test]
fn resolve_user_node_weight_uses_global_when_node_inherits() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let node_id = store.list_nodes()[0].node_id.clone();
    let user = store.create_user("alice".to_string(), None).unwrap();

    DesiredStateCommand::SetUserGlobalWeight {
        user_id: user.user_id.clone(),
        weight: 321,
    }
    .apply(store.state_mut())
    .unwrap();
    DesiredStateCommand::SetUserNodeWeight {
        user_id: user.user_id.clone(),
        node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
        weight: 999,
    }
    .apply(store.state_mut())
    .unwrap();
    DesiredStateCommand::SetNodeWeightPolicy {
        node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
        inherit_global: true,
    }
    .apply(store.state_mut())
    .unwrap();

    assert_eq!(store.resolve_user_node_weight(&user.user_id, &node_id), 321);
}

#[test]
fn resolve_user_node_weight_uses_node_override_when_inherit_disabled() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let node_id = store.list_nodes()[0].node_id.clone();
    let user = store.create_user("alice".to_string(), None).unwrap();

    DesiredStateCommand::SetUserGlobalWeight {
        user_id: user.user_id.clone(),
        weight: 321,
    }
    .apply(store.state_mut())
    .unwrap();
    DesiredStateCommand::SetNodeWeightPolicy {
        node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
        inherit_global: false,
    }
    .apply(store.state_mut())
    .unwrap();

    // Without explicit node weight, node-local override falls back to global.
    assert_eq!(store.resolve_user_node_weight(&user.user_id, &node_id), 321);

    DesiredStateCommand::SetUserNodeWeight {
        user_id: user.user_id.clone(),
        node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
        weight: 999,
    }
    .apply(store.state_mut())
    .unwrap();
    assert_eq!(store.resolve_user_node_weight(&user.user_id, &node_id), 999);
}

#[test]
fn desired_state_apply_ensure_membership_is_idempotent() {
    let mut state = PersistedState::empty();
    state.users.insert(
        "user_1".to_string(),
        User {
            user_id: "user_1".to_string(),
            display_name: "alice".to_string(),
            subscription_token: xp_test_fixtures::label_sub1().to_owned(),
            credential_epoch: 0,
            priority_tier: Default::default(),
            quota_reset: UserQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_1".to_string(),
        Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_fixture518().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 443,
            meta: json!({}),
        },
    );

    let out = DesiredStateCommand::EnsureMembership {
        user_id: "user_1".to_string(),
        endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
    };
    assert_eq!(
        out.apply(&mut state).unwrap(),
        DesiredStateApplyResult::Applied
    );
    assert_eq!(
        out.apply(&mut state).unwrap(),
        DesiredStateApplyResult::Applied
    );
    assert_eq!(state.node_user_endpoint_memberships.len(), 1);
}

#[test]
fn desired_state_apply_bump_user_credential_epoch_increments_and_returns_epoch() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let user = store.create_user("alice".to_string(), None).unwrap();
    assert_eq!(store.get_user(&user.user_id).unwrap().credential_epoch, 0);

    let out = DesiredStateCommand::BumpUserCredentialEpoch {
        user_id: user.user_id.clone(),
    }
    .apply(store.state_mut())
    .unwrap();
    let DesiredStateApplyResult::UserCredentialEpochBumped {
        user_id: out_user_id,
        credential_epoch,
    } = out
    else {
        panic!("expected UserCredentialEpochBumped");
    };
    assert_eq!(out_user_id, user.user_id);
    assert_eq!(credential_epoch, 1);
    assert_eq!(store.get_user(&user.user_id).unwrap().credential_epoch, 1);
}
#[test]
fn load_or_init_migrates_v10_geo_db_settings_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let mut legacy = serde_json::to_value(PersistedState::empty()).unwrap();
    legacy["schema_version"] = serde_json::json!(SCHEMA_VERSION_V10);
    legacy
        .as_object_mut()
        .unwrap()
        .remove("geo_db_update_settings");
    std::fs::write(
        tmp.path().join("state.json"),
        serde_json::to_vec_pretty(&legacy).unwrap(),
    )
    .unwrap();

    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    assert_eq!(store.state().schema_version, SCHEMA_VERSION);
}

#[test]
fn load_or_init_prunes_deleted_probe_nodes_from_current_schema_state() {
    let tmp = tempfile::tempdir().unwrap();
    let raw = probe_state_with_stale_deleted_node();
    std::fs::write(
        tmp.path().join("state.json"),
        serde_json::to_vec_pretty(&raw).unwrap(),
    )
    .unwrap();

    let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    assert_eq!(
        store
            .state()
            .endpoint_probe_participants_by_hour
            .get("2026-03-11T11:00:00Z"),
        Some(&BTreeSet::from(["node_keep".to_string()])),
    );
    let bucket = store
        .state()
        .endpoint_probe_history
        .get("endpoint_1")
        .and_then(|history| history.hours.get("2026-03-11T11:00:00Z"))
        .expect("endpoint probe bucket should survive for the kept node");
    assert_eq!(
        bucket.by_node.keys().cloned().collect::<Vec<_>>(),
        vec!["node_keep".to_string()],
    );

    let saved: PersistedState =
        serde_json::from_slice(&fs::read(tmp.path().join("state.json")).unwrap()).unwrap();
    assert_eq!(
        saved
            .endpoint_probe_participants_by_hour
            .get("2026-03-11T11:00:00Z"),
        Some(&BTreeSet::from(["node_keep".to_string()])),
    );
}

#[test]
fn desired_state_apply_append_endpoint_probe_samples_registers_participant_even_when_empty() {
    let mut state = PersistedState::empty();
    state.endpoints.insert(
        "endpoint_1".to_string(),
        Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_fixture518().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 443,
            meta: json!({}),
        },
    );

    DesiredStateCommand::AppendEndpointProbeSamples {
        hour: xp_test_fixtures::probe_hour().to_owned(),
        from_node_id: xp_test_fixtures::node_id_fixture32().to_owned(),
        samples: Vec::new(),
    }
    .apply(&mut state)
    .unwrap();

    assert_eq!(
        state
            .endpoint_probe_participants_by_hour
            .get("2026-03-11T11:00:00Z"),
        Some(&BTreeSet::from([
            xp_test_fixtures::node_id_fixture32().to_owned()
        ])),
    );
    assert!(state.endpoint_probe_history.is_empty());
}

#[test]
fn desired_state_apply_append_endpoint_probe_samples_prunes_participants_and_history() {
    let mut state = PersistedState::empty();
    state.endpoints.insert(
        xp_test_fixtures::label_endpoint1().to_owned(),
        Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
            node_id: xp_test_fixtures::label_node1().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_fixture518().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 443,
            meta: json!({}),
        },
    );

    let sample = EndpointProbeAppendSample {
        endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
        ok: true,
        skipped: false,
        checked_at: xp_test_fixtures::timestamp_at20260308_t003000_z().to_owned(),
        latency_ms: Some(xp_test_fixtures::number_value22()),
        target_id: None,
        target_url: None,
        error: None,
        config_hash: xp_test_fixtures::primary_probe_config_hash().to_owned(),
    };
    let commands = [
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t000400000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture17().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t000500000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture32().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t000600000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture36().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t000700000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture56().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t000800000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture57().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t000900000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture63().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t001000000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture69().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t001100000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture70().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t001200000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture72().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t001300000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture73().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t001400000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture77().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t001500000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture93().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t001600000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture98().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t002100000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture106().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t002200000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture110().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t002300000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture113().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20260308_t000000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture118().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20260308_t000200_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture124().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20260308_t000100_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture134().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20260307_t010000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture145().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20260308_t005900_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture149().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20260308_t005800_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture153().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20260131_t000000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture182().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20240101_t012100000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture187().to_owned(),
            samples: vec![sample.clone()],
        },
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: xp_test_fixtures::timestamp_at20260301_t000000_z().to_owned(),
            from_node_id: xp_test_fixtures::node_id_fixture188().to_owned(),
            samples: vec![sample],
        },
    ];
    for command in commands {
        command.apply(&mut state).unwrap();
    }

    let history = state
        .endpoint_probe_history
        .get(xp_test_fixtures::label_endpoint1())
        .expect("endpoint history");
    assert_eq!(history.hours.len(), ENDPOINT_PROBE_HOUR_BUCKET_LIMIT);
    assert_eq!(
        state.endpoint_probe_participants_by_hour.len(),
        ENDPOINT_PROBE_HOUR_BUCKET_LIMIT
    );
    assert!(
        !history
            .hours
            .contains_key(xp_test_fixtures::timestamp_at20240101_t000400000_z())
    );
    assert!(
        history
            .hours
            .contains_key(xp_test_fixtures::timestamp_at20260308_t000200_z())
    );
    assert!(
        !state
            .endpoint_probe_participants_by_hour
            .contains_key(xp_test_fixtures::timestamp_at20240101_t000400000_z())
    );
    assert!(
        state
            .endpoint_probe_participants_by_hour
            .contains_key(xp_test_fixtures::timestamp_at20260308_t000200_z())
    );
}

#[test]
fn endpoint_probe_participants_for_hour_unions_participant_map_and_legacy_samples() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
    let node_id = store.list_nodes()[0].node_id.clone();
    let endpoint = store
        .create_endpoint(
            node_id,
            EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            443,
            json!({}),
        )
        .unwrap();
    let second_endpoint = store
        .create_endpoint(
            endpoint.node_id.clone(),
            EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            8443,
            json!({}),
        )
        .unwrap();

    let hour = "2026-03-11T11:00:00Z".to_string();
    store
        .state_mut()
        .endpoint_probe_participants_by_hour
        .insert(hour.clone(), BTreeSet::from(["node_explicit".to_string()]));
    store
        .state_mut()
        .endpoint_probe_history
        .entry(endpoint.endpoint_id)
        .or_default()
        .hours
        .entry(hour.clone())
        .or_default()
        .by_node
        .insert(
            "node_from_history_a".to_string(),
            EndpointProbeNodeSample {
                ok: true,
                skipped: false,
                checked_at: xp_test_fixtures::timestamp_at20240101_t090100_z().to_owned(),
                latency_ms: xp_test_fixtures::number_value24(),
                target_id: None,
                target_url: None,
                error: None,
                config_hash: xp_test_fixtures::primary_probe_config_hash().to_owned(),
            },
        );
    store
        .state_mut()
        .endpoint_probe_history
        .entry(second_endpoint.endpoint_id)
        .or_default()
        .hours
        .entry(hour.clone())
        .or_default()
        .by_node
        .insert(
            "node_from_history_b".to_string(),
            EndpointProbeNodeSample {
                ok: false,
                skipped: false,
                checked_at: xp_test_fixtures::timestamp_at20240101_t090200_z().to_owned(),
                latency_ms: xp_test_fixtures::none(),
                target_id: None,
                target_url: None,
                error: Some("dial failed".to_string()),
                config_hash: xp_test_fixtures::primary_probe_config_hash().to_owned(),
            },
        );

    assert_eq!(
        store.endpoint_probe_participants_for_hour(&hour),
        BTreeSet::from([
            "node_explicit".to_string(),
            "node_from_history_a".to_string(),
            "node_from_history_b".to_string(),
        ])
    );
}

#[test]
fn desired_state_apply_delete_node_removes_probe_participation_for_removed_node() {
    let mut state = PersistedState::empty();
    state.nodes.insert(
        "node_keep".to_string(),
        Node {
            node_id: xp_test_fixtures::label_node_keep().to_owned(),
            node_name: xp_test_fixtures::label_keep().to_owned(),
            access_host: xp_test_fixtures::host_fixture516().to_owned(),
            api_base_url: xp_test_fixtures::service_fixture517().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.nodes.insert(
        "node_drop".to_string(),
        Node {
            node_id: xp_test_fixtures::label_node_drop().to_owned(),
            node_name: xp_test_fixtures::label_drop().to_owned(),
            access_host: xp_test_fixtures::host_fixture544().to_owned(),
            api_base_url: xp_test_fixtures::service_fixture545().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_1".to_string(),
        Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
            node_id: xp_test_fixtures::label_node_keep().to_owned(),
            tag: xp_test_fixtures::endpoint_tag_fixture518().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 443,
            meta: json!({}),
        },
    );
    state.endpoint_probe_participants_by_hour.insert(
        "2026-03-11T11:00:00Z".to_string(),
        BTreeSet::from(["node_keep".to_string(), "node_drop".to_string()]),
    );
    state
        .endpoint_probe_history
        .entry("endpoint_1".to_string())
        .or_default()
        .hours
        .entry("2026-03-11T11:00:00Z".to_string())
        .or_default()
        .by_node
        .insert(
            "node_drop".to_string(),
            EndpointProbeNodeSample {
                ok: true,
                skipped: false,
                checked_at: xp_test_fixtures::timestamp_at20240101_t090100_z().to_owned(),
                latency_ms: xp_test_fixtures::number_value24(),
                target_id: None,
                target_url: None,
                error: None,
                config_hash: xp_test_fixtures::primary_probe_config_hash().to_owned(),
            },
        );

    DesiredStateCommand::DeleteNode {
        node_id: xp_test_fixtures::label_node_drop().to_owned(),
        delete_endpoints: false,
        expected_endpoint_ids: Vec::new(),
    }
    .apply(&mut state)
    .unwrap();

    assert_eq!(
        state
            .endpoint_probe_participants_by_hour
            .get("2026-03-11T11:00:00Z"),
        Some(&BTreeSet::from(["node_keep".to_string()])),
    );
    assert!(state.endpoint_probe_history.is_empty());
}

#[test]
fn desired_state_apply_delete_node_can_delete_referenced_endpoints() {
    let mut state = PersistedState::empty();
    state.nodes.insert(
        "node_drop".to_string(),
        Node {
            node_id: xp_test_fixtures::label_node_drop().to_owned(),
            node_name: xp_test_fixtures::label_node_drop().to_owned(),
            access_host: xp_test_fixtures::host_fixture546().to_owned(),
            api_base_url: xp_test_fixtures::service_fixture547().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_drop".to_string(),
        Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint_drop().to_owned(),
            node_id: xp_test_fixtures::label_node_drop().to_owned(),
            tag: xp_test_fixtures::label_endpoint_drop_variant2().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 8388,
            meta: serde_json::json!({}),
        },
    );
    state
        .endpoint_probe_history
        .entry("endpoint_drop".to_string())
        .or_default();

    let out = DesiredStateCommand::DeleteNode {
        node_id: xp_test_fixtures::label_node_drop().to_owned(),
        delete_endpoints: true,
        expected_endpoint_ids: vec!["endpoint_drop".to_string()],
    }
    .apply(&mut state)
    .unwrap();

    assert_eq!(
        out,
        DesiredStateApplyResult::NodeDeleted {
            deleted: true,
            deleted_endpoint_tags: vec!["endpoint-drop".to_string()],
        },
    );
    assert!(!state.nodes.contains_key("node_drop"));
    assert!(!state.endpoints.contains_key("endpoint_drop"));
    assert!(!state.endpoint_probe_history.contains_key("endpoint_drop"));
}

#[test]
fn desired_state_apply_delete_node_rejects_changed_endpoint_set() {
    let mut state = PersistedState::empty();
    state.nodes.insert(
        "node_drop".to_string(),
        Node {
            node_id: xp_test_fixtures::label_node_drop().to_owned(),
            node_name: xp_test_fixtures::label_node_drop().to_owned(),
            access_host: xp_test_fixtures::host_fixture546().to_owned(),
            api_base_url: xp_test_fixtures::service_fixture547().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_new".to_string(),
        Endpoint {
            endpoint_id: xp_test_fixtures::label_endpoint_new().to_owned(),
            node_id: xp_test_fixtures::label_node_drop().to_owned(),
            tag: xp_test_fixtures::label_endpoint_new_variant2().to_owned(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 8388,
            meta: serde_json::json!({}),
        },
    );

    let err = DesiredStateCommand::DeleteNode {
        node_id: xp_test_fixtures::label_node_drop().to_owned(),
        delete_endpoints: true,
        expected_endpoint_ids: vec!["endpoint_previewed".to_string()],
    }
    .apply(&mut state)
    .unwrap_err();

    assert!(matches!(
        err,
        StoreError::Domain(crate::domain::DomainError::NodeEndpointSetChanged {
            node_id
        }) if node_id == "node_drop"
    ));
    assert!(state.nodes.contains_key("node_drop"));
    assert!(state.endpoints.contains_key("endpoint_new"));
}

#[test]
fn desired_state_apply_delete_node_rejects_removed_preview_endpoint_set() {
    let mut state = PersistedState::empty();
    state.nodes.insert(
        "node_drop".to_string(),
        Node {
            node_id: xp_test_fixtures::label_node_drop().to_owned(),
            node_name: xp_test_fixtures::label_node_drop().to_owned(),
            access_host: xp_test_fixtures::host_fixture546().to_owned(),
            api_base_url: xp_test_fixtures::service_fixture547().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );

    let err = DesiredStateCommand::DeleteNode {
        node_id: xp_test_fixtures::label_node_drop().to_owned(),
        delete_endpoints: true,
        expected_endpoint_ids: vec!["endpoint_previewed".to_string()],
    }
    .apply(&mut state)
    .unwrap_err();

    assert!(matches!(
        err,
        StoreError::Domain(crate::domain::DomainError::NodeEndpointSetChanged {
            node_id
        }) if node_id == "node_drop"
    ));
    assert!(state.nodes.contains_key("node_drop"));
}

#[test]
fn desired_state_apply_set_geo_db_update_settings_is_noop() {
    let mut state = PersistedState::empty();
    let before = state.clone();
    let result = DesiredStateCommand::SetGeoDbUpdateSettings {
        settings: GeoDbUpdateSettingsCompat {
            provider: "legacy".to_string(),
            auto_update_enabled: true,
            update_interval_days: 7,
        },
    }
    .apply(&mut state)
    .unwrap();
    assert_eq!(result, DesiredStateApplyResult::Applied);
    assert_eq!(state, before);
}

mod endpoint_meta;
