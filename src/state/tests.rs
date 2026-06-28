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
        bootstrap_node_id: None,
        bootstrap_node_name: "node-1".to_string(),
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: "https://127.0.0.1:62416".to_string(),
    }
}

fn test_user(user_id: &str) -> User {
    User {
        user_id: user_id.to_string(),
        display_name: user_id.to_string(),
        subscription_token: format!("sub_{user_id}"),
        credential_epoch: 0,
        priority_tier: UserPriorityTier::P2,
        quota_reset: UserQuotaReset::default(),
    }
}

fn test_node(node_id: &str) -> Node {
    Node {
        node_id: node_id.to_string(),
        node_name: node_id.to_string(),
        access_host: "localhost".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    }
}

fn ss_endpoint(endpoint_id: &str, node_id: &str) -> Endpoint {
    Endpoint {
        endpoint_id: endpoint_id.to_string(),
        node_id: node_id.to_string(),
        tag: endpoint_id.to_string(),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port: 10_000,
        meta: json!({}),
    }
}

fn vless_endpoint(endpoint_id: &str, node_id: &str) -> Endpoint {
    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: "example.com:443".to_string(),
            server_names: vec!["example.com".to_string()],
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
        managed_default: false,
    };

    Endpoint {
        endpoint_id: endpoint_id.to_string(),
        node_id: node_id.to_string(),
        tag: endpoint_id.to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::to_value(meta).unwrap(),
    }
}

fn probe_state_with_stale_deleted_node() -> PersistedState {
    let mut state = PersistedState::empty();
    state.nodes.insert(
        "node_keep".to_string(),
        Node {
            node_id: "node_keep".to_string(),
            node_name: "keep".to_string(),
            access_host: "keep.example.com".to_string(),
            api_base_url: "https://keep.example.com".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_1".to_string(),
        Endpoint {
            endpoint_id: "endpoint_1".to_string(),
            node_id: "node_keep".to_string(),
            tag: "ss2022-endpoint_1".to_string(),
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
            checked_at: "2026-03-11T11:05:00Z".to_string(),
            latency_ms: Some(120),
            target_id: None,
            target_url: None,
            error: None,
            config_hash: "cfg".to_string(),
        },
    );
    bucket.by_node.insert(
        "node_drop".to_string(),
        EndpointProbeNodeSample {
            ok: true,
            skipped: false,
            checked_at: "2026-03-11T11:06:00Z".to_string(),
            latency_ms: Some(140),
            target_id: None,
            target_url: None,
            error: None,
            config_hash: "cfg".to_string(),
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
            "node_1": {
              "node_id": "node_1",
              "node_name": "node-1",
              "public_domain": "example.com",
              "api_base_url": "https://127.0.0.1:62416"
            }
          }
        }))
        .unwrap(),
    )
    .unwrap();

    let store = JsonSnapshotStore::load_or_init(StoreInit {
        data_dir: data_dir.to_path_buf(),
        bootstrap_node_id: None,
        bootstrap_node_name: "node-1".to_string(),
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: "https://127.0.0.1:62416".to_string(),
    })
    .unwrap();

    assert_eq!(store.state().schema_version, SCHEMA_VERSION);
    let node = store.state().nodes.get("node_1").unwrap();
    assert_eq!(node.access_host, "example.com");

    let bytes = fs::read(&state_path).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved["schema_version"], SCHEMA_VERSION);
    assert!(saved["nodes"]["node_1"].get("access_host").is_some());
    assert!(saved["nodes"]["node_1"].get("public_domain").is_none());
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
                cycle_start_at: "2026-01-01T00:00:00Z".to_string(),
                cycle_end_at: "2026-02-01T00:00:00Z".to_string(),
                used_bytes: 10,
                last_uplink_total: 10,
                last_downlink_total: 0,
                last_seen_at: "2026-01-01T00:00:10Z".to_string(),
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
            node_id: node_id.clone(),
            node_name: "node-1".to_string(),
            access_host: "".to_string(),
            api_base_url: "https://127.0.0.1:62416".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        endpoint_id.clone(),
        Endpoint {
            endpoint_id: endpoint_id.clone(),
            node_id: node_id.clone(),
            tag: "e1".to_string(),
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
            subscription_token: "sub_1".to_string(),
            credential_epoch: 0,
            priority_tier: UserPriorityTier::P2,
            quota_reset: UserQuotaReset::default(),
        },
    );
    state
        .node_user_endpoint_memberships
        .insert(NodeUserEndpointMembership {
            user_id: user_id.clone(),
            node_id: node_id.clone(),
            endpoint_id: endpoint_id.clone(),
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
            node_id: "node-1".to_string(),
            node_name: "Tokyo".to_string(),
            access_host: "tokyo.example.com".to_string(),
            api_base_url: "https://tokyo.example.com".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    let probe = NodeEgressProbeState {
        selected_public_ip: Some("203.0.113.8".to_string()),
        subscription_region: NodeSubscriptionRegion::Taiwan,
        checked_at: "2026-04-24T00:00:00Z".to_string(),
        last_success_at: Some("2026-04-24T00:00:00Z".to_string()),
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
        endpoint_id: "ss_1".to_string(),
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

    let endpoint_id = "endpoint_1".to_string();
    let node_id = "node_1".to_string();

    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: "ignored.example.com:443".to_string(),
            server_names: vec![
                " b.example.com ".to_string(),
                "a.example.com".to_string(),
                "B.example.com".to_string(),
            ],
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
        managed_default: false,
    };

    let endpoint = Endpoint {
        endpoint_id: endpoint_id.clone(),
        node_id,
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

    assert_eq!(
        meta.reality.server_names,
        vec!["b.example.com".to_string(), "a.example.com".to_string()]
    );
    assert_eq!(meta.reality.dest, "ignored.example.com:443");
}

#[test]
fn upsert_vless_endpoint_manual_rejects_invalid_dest() {
    let mut state = PersistedState::empty();

    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: String::new(),
            server_names: vec!["example.com".to_string()],
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
        managed_default: false,
    };

    let endpoint = Endpoint {
        endpoint_id: "endpoint_1".to_string(),
        node_id: "node_1".to_string(),
        tag: "vless-test".to_string(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: 443,
        meta: serde_json::to_value(meta).unwrap(),
    };

    let err = DesiredStateCommand::UpsertEndpoint { endpoint }
        .apply(&mut state)
        .unwrap_err();
    assert!(err.to_string().contains("dest is required"));
    assert!(state.endpoints.is_empty());
}

#[test]
fn upsert_vless_endpoint_manual_accepts_tcp_prefixed_dest() {
    let mut state = PersistedState::empty();

    let endpoint_id = "endpoint_1".to_string();
    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: "tcp://oneclient.sfx.ms:443".to_string(),
            server_names: vec!["public.sn.files.1drv.com".to_string()],
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
    assert_eq!(meta.reality.dest, "tcp://oneclient.sfx.ms:443");
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

    let endpoint_id = "endpoint_1".to_string();

    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: String::new(),
            server_names: vec![],
            server_names_source: RealityServerNamesSource::Global,
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

    let endpoint_id = "endpoint_1".to_string();

    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: "127.0.0.1:39043".to_string(),
            server_names: vec![],
            server_names_source: RealityServerNamesSource::Global,
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
        managed_default: true,
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

    assert_eq!(
        meta.reality.server_names,
        vec![
            "first.example.com".to_string(),
            "third.example.com".to_string()
        ]
    );
    assert_eq!(meta.reality.dest, "127.0.0.1:39043");
}

#[test]
fn rotate_vless_reality_short_id_updates_meta_and_persists() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();

    let node_id = store.list_nodes()[0].node_id.clone();
    let endpoint_id = "endpoint_1".to_string();
    let kind = EndpointKind::VlessRealityVisionTcp;

    let meta = VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: "example.com:443".to_string(),
            server_names: vec!["example.com".to_string()],
            server_names_source: Default::default(),
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
        managed_default: false,
    };

    store.state_mut().endpoints.insert(
        endpoint_id.clone(),
        Endpoint {
            endpoint_id: endpoint_id.clone(),
            node_id,
            tag: endpoint_tag(&kind, &endpoint_id),
            kind,
            port: 443,
            meta: serde_json::to_value(meta).unwrap(),
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
    let meta: VlessRealityVisionTcpEndpointMeta = serde_json::from_value(endpoint.meta).unwrap();

    assert_eq!(out.active_short_id, meta.active_short_id);
    assert_eq!(out.short_ids, meta.short_ids);
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
                    endpoint_id: endpoint.endpoint_id,
                    endpoint_tag: endpoint.tag,
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
                    node_id: node_id.clone(),
                    endpoint_id: endpoint.endpoint_id.clone(),
                    endpoint_tag: endpoint.tag.clone(),
                    ips: vec!["203.0.113.7".to_string()],
                },
                crate::inbound_ip_usage::InboundIpMinuteSample {
                    membership_key: "stale-user::stale-endpoint".to_string(),
                    user_id: "stale-user".to_string(),
                    node_id,
                    endpoint_id: "stale-endpoint".to_string(),
                    endpoint_tag: "stale-tag".to_string(),
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
        node_id: "node_1".to_string(),
        node_name: "node-1".to_string(),
        access_host: "example.com".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
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
        endpoint_id: "ep_1".to_string(),
        node_id: "node_1".to_string(),
        tag: "ss2022-ep_1".to_string(),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port: 443,
        meta: json!({"k":"v"}),
    };

    DesiredStateCommand::UpsertEndpoint {
        endpoint: endpoint.clone(),
    }
    .apply(&mut state)
    .unwrap();
    assert_eq!(state.endpoints.get(&endpoint.endpoint_id), Some(&endpoint));

    let out = DesiredStateCommand::DeleteEndpoint {
        endpoint_id: endpoint.endpoint_id.clone(),
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
        endpoint_id: "ep_1".to_string(),
        node_id: "node_1".to_string(),
        tag: "ss2022-ep_1".to_string(),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port: 0,
        meta: json!({}),
    };

    let err = DesiredStateCommand::UpsertEndpoint { endpoint }
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
        subscription_token: "sub_1".to_string(),
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
        subscription_token: "sub_2".to_string(),
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
        node_id: node_id.clone(),
        weight: 999,
    }
    .apply(store.state_mut())
    .unwrap();
    DesiredStateCommand::SetNodeWeightPolicy {
        node_id: node_id.clone(),
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
        node_id: node_id.clone(),
        inherit_global: false,
    }
    .apply(store.state_mut())
    .unwrap();

    // Without explicit node weight, node-local override falls back to global.
    assert_eq!(store.resolve_user_node_weight(&user.user_id, &node_id), 321);

    DesiredStateCommand::SetUserNodeWeight {
        user_id: user.user_id.clone(),
        node_id: node_id.clone(),
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
            subscription_token: "sub_1".to_string(),
            credential_epoch: 0,
            priority_tier: Default::default(),
            quota_reset: UserQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_1".to_string(),
        Endpoint {
            endpoint_id: "endpoint_1".to_string(),
            node_id: "node_1".to_string(),
            tag: "ss2022-endpoint_1".to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 443,
            meta: json!({}),
        },
    );

    let out = DesiredStateCommand::EnsureMembership {
        user_id: "user_1".to_string(),
        endpoint_id: "endpoint_1".to_string(),
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
            endpoint_id: "endpoint_1".to_string(),
            node_id: "node_1".to_string(),
            tag: "ss2022-endpoint_1".to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 443,
            meta: json!({}),
        },
    );

    DesiredStateCommand::AppendEndpointProbeSamples {
        hour: "2026-03-11T11:00:00Z".to_string(),
        from_node_id: "node_2".to_string(),
        samples: Vec::new(),
    }
    .apply(&mut state)
    .unwrap();

    assert_eq!(
        state
            .endpoint_probe_participants_by_hour
            .get("2026-03-11T11:00:00Z"),
        Some(&BTreeSet::from(["node_2".to_string()])),
    );
    assert!(state.endpoint_probe_history.is_empty());
}

#[test]
fn desired_state_apply_append_endpoint_probe_samples_prunes_participants_and_history() {
    let mut state = PersistedState::empty();
    state.endpoints.insert(
        "endpoint_1".to_string(),
        Endpoint {
            endpoint_id: "endpoint_1".to_string(),
            node_id: "node_1".to_string(),
            tag: "ss2022-endpoint_1".to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 443,
            meta: json!({}),
        },
    );

    for hour_idx in 0..25 {
        let hour = format!("2026-03-{:02}T00:00:00Z", hour_idx + 1);
        DesiredStateCommand::AppendEndpointProbeSamples {
            hour: hour.clone(),
            from_node_id: format!("node_{}", hour_idx + 1),
            samples: vec![EndpointProbeAppendSample {
                endpoint_id: "endpoint_1".to_string(),
                ok: true,
                skipped: false,
                checked_at: format!("2026-03-{:02}T00:30:00Z", hour_idx + 1),
                latency_ms: Some(100 + hour_idx as u32),
                target_id: None,
                target_url: None,
                error: None,
                config_hash: "cfg".to_string(),
            }],
        }
        .apply(&mut state)
        .unwrap();
    }

    let history = state
        .endpoint_probe_history
        .get("endpoint_1")
        .expect("endpoint history");
    assert_eq!(history.hours.len(), ENDPOINT_PROBE_HOUR_BUCKET_LIMIT);
    assert_eq!(
        state.endpoint_probe_participants_by_hour.len(),
        ENDPOINT_PROBE_HOUR_BUCKET_LIMIT
    );
    assert!(!history.hours.contains_key("2026-03-01T00:00:00Z"));
    assert!(history.hours.contains_key("2026-03-25T00:00:00Z"));
    assert!(
        !state
            .endpoint_probe_participants_by_hour
            .contains_key("2026-03-01T00:00:00Z")
    );
    assert!(
        state
            .endpoint_probe_participants_by_hour
            .contains_key("2026-03-25T00:00:00Z")
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
                checked_at: "2026-03-11T11:10:00Z".to_string(),
                latency_ms: Some(123),
                target_id: None,
                target_url: None,
                error: None,
                config_hash: "cfg".to_string(),
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
                checked_at: "2026-03-11T11:12:00Z".to_string(),
                latency_ms: None,
                target_id: None,
                target_url: None,
                error: Some("dial failed".to_string()),
                config_hash: "cfg".to_string(),
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
            node_id: "node_keep".to_string(),
            node_name: "keep".to_string(),
            access_host: "keep.example.com".to_string(),
            api_base_url: "https://keep.example.com".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.nodes.insert(
        "node_drop".to_string(),
        Node {
            node_id: "node_drop".to_string(),
            node_name: "drop".to_string(),
            access_host: "drop.example.com".to_string(),
            api_base_url: "https://drop.example.com".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_1".to_string(),
        Endpoint {
            endpoint_id: "endpoint_1".to_string(),
            node_id: "node_keep".to_string(),
            tag: "ss2022-endpoint_1".to_string(),
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
                checked_at: "2026-03-11T11:10:00Z".to_string(),
                latency_ms: Some(123),
                target_id: None,
                target_url: None,
                error: None,
                config_hash: "cfg".to_string(),
            },
        );

    DesiredStateCommand::DeleteNode {
        node_id: "node_drop".to_string(),
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
            node_id: "node_drop".to_string(),
            node_name: "node_drop".to_string(),
            access_host: "node-drop.example.invalid".to_string(),
            api_base_url: "https://node-drop.example.invalid".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_drop".to_string(),
        Endpoint {
            endpoint_id: "endpoint_drop".to_string(),
            node_id: "node_drop".to_string(),
            tag: "endpoint-drop".to_string(),
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
        node_id: "node_drop".to_string(),
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
            node_id: "node_drop".to_string(),
            node_name: "node_drop".to_string(),
            access_host: "node-drop.example.invalid".to_string(),
            api_base_url: "https://node-drop.example.invalid".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );
    state.endpoints.insert(
        "endpoint_new".to_string(),
        Endpoint {
            endpoint_id: "endpoint_new".to_string(),
            node_id: "node_drop".to_string(),
            tag: "endpoint-new".to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 8388,
            meta: serde_json::json!({}),
        },
    );

    let err = DesiredStateCommand::DeleteNode {
        node_id: "node_drop".to_string(),
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
            node_id: "node_drop".to_string(),
            node_name: "node_drop".to_string(),
            access_host: "node-drop.example.invalid".to_string(),
            api_base_url: "https://node-drop.example.invalid".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    );

    let err = DesiredStateCommand::DeleteNode {
        node_id: "node_drop".to_string(),
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
