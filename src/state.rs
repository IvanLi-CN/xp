use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{
        DomainError, Endpoint, EndpointKind, Grant, GrantCredentials, Node, NodeQuotaReset,
        QuotaResetSource, RealityDomain, Ss2022Credentials, User, UserNodeQuota, UserQuotaReset,
        VlessCredentials, validate_cycle_day_of_month, validate_group_name, validate_port,
        validate_tz_offset_minutes,
    },
    id::new_ulid_string,
    protocol::{
        RealityKeys, RealityServerNamesSource, RotateShortIdResult,
        SS2022_METHOD_2022_BLAKE3_AES_128_GCM, Ss2022EndpointMeta,
        VlessRealityVisionTcpEndpointMeta, generate_reality_keypair, generate_short_id_16hex,
        generate_ss2022_psk_b64, rotate_short_ids_in_place, ss2022_password,
        validate_reality_server_name,
    },
};

pub const SCHEMA_VERSION: u32 = 5;
const SCHEMA_VERSION_V4: u32 = 4;
pub const USAGE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct StoreInit {
    pub data_dir: PathBuf,
    pub bootstrap_node_id: Option<String>,
    pub bootstrap_node_name: String,
    pub bootstrap_access_host: String,
    pub bootstrap_api_base_url: String,
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    SerdeJson(serde_json::Error),
    Domain(DomainError),
    Migration { message: String },
    SchemaVersionMismatch { expected: u32, got: u32 },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::SerdeJson(e) => write!(f, "json error: {e}"),
            Self::Domain(e) => write!(f, "{e}"),
            Self::Migration { message } => write!(f, "migration error: {message}"),
            Self::SchemaVersionMismatch { expected, got } => {
                write!(f, "schema_version mismatch: expected {expected}, got {got}")
            }
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::SerdeJson(e) => Some(e),
            Self::Domain(e) => Some(e),
            Self::Migration { .. } => None,
            Self::SchemaVersionMismatch { .. } => None,
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::SerdeJson(value)
    }
}

impl From<DomainError> for StoreError {
    fn from(value: DomainError) -> Self {
        Self::Domain(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedState {
    pub schema_version: u32,
    #[serde(default)]
    pub nodes: BTreeMap<String, Node>,
    #[serde(default)]
    pub endpoints: BTreeMap<String, Endpoint>,
    /// Endpoint probe history (hour buckets, per node).
    ///
    /// Keyed by `endpoint_id`.
    #[serde(default)]
    pub endpoint_probe_history: BTreeMap<String, EndpointProbeHistory>,
    #[serde(default)]
    pub users: BTreeMap<String, User>,
    #[serde(default)]
    pub grants: BTreeMap<String, Grant>,
    #[serde(default)]
    pub reality_domains: Vec<RealityDomain>,
    #[serde(default)]
    pub user_node_quotas: BTreeMap<String, BTreeMap<String, UserNodeQuotaConfig>>,
}

impl PersistedState {
    pub fn empty() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            nodes: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            endpoint_probe_history: BTreeMap::new(),
            users: BTreeMap::new(),
            grants: BTreeMap::new(),
            reality_domains: Vec::new(),
            user_node_quotas: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointProbeHistory {
    /// Keyed by an hour key like `2026-02-07T12:00:00Z`.
    #[serde(default)]
    pub hours: BTreeMap<String, EndpointProbeHour>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointProbeHour {
    /// Keyed by `node_id`.
    #[serde(default)]
    pub by_node: BTreeMap<String, EndpointProbeNodeSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointProbeNodeSample {
    pub ok: bool,
    pub checked_at: String,
    #[serde(default)]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub target_url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// Hash of the probe configuration to ensure cluster-wide consistency.
    pub config_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointProbeAppendSample {
    pub endpoint_id: String,
    pub ok: bool,
    pub checked_at: String,
    #[serde(default)]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub target_url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    pub config_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserNodeQuotaConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_limit_bytes: Option<u64>,
    #[serde(default)]
    pub quota_reset_source: QuotaResetSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CyclePolicyDefaultV2 {
    ByUser,
    ByNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CyclePolicyV2 {
    InheritUser,
    ByUser,
    ByNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct UserV2 {
    user_id: String,
    display_name: String,
    subscription_token: String,
    cycle_policy_default: CyclePolicyDefaultV2,
    cycle_day_of_month_default: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct GrantV2 {
    grant_id: String,
    user_id: String,
    endpoint_id: String,
    #[serde(default)]
    group_name: Option<String>,
    enabled: bool,
    quota_limit_bytes: u64,
    cycle_policy: CyclePolicyV2,
    cycle_day_of_month: Option<u8>,
    note: Option<String>,
    credentials: GrantCredentials,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PersistedStateV2Like {
    schema_version: u32,
    #[serde(default)]
    nodes: BTreeMap<String, Node>,
    #[serde(default)]
    endpoints: BTreeMap<String, Endpoint>,
    #[serde(default)]
    users: BTreeMap<String, UserV2>,
    #[serde(default)]
    grants: BTreeMap<String, GrantV2>,
    #[serde(default)]
    user_node_quotas: BTreeMap<String, BTreeMap<String, u64>>,
}

fn sanitize_group_name_fragment(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    out
}

fn make_legacy_group_name(user_id: &str) -> String {
    let fragment = sanitize_group_name_fragment(user_id);
    let mut out = format!("legacy-{fragment}");
    if out.len() > 64 {
        out.truncate(64);
    }
    if out.is_empty() {
        out = "legacy".to_string();
    }
    out
}

fn migrate_v2_like_to_v3(input: PersistedStateV2Like) -> Result<PersistedState, StoreError> {
    let PersistedStateV2Like {
        schema_version: _,
        nodes,
        endpoints,
        users,
        grants,
        user_node_quotas,
    } = input;

    let users_v2 = users;

    let mut out = PersistedState::empty();
    out.schema_version = SCHEMA_VERSION_V4;
    out.nodes = nodes;
    out.endpoints = endpoints;

    // Users: cycle_* -> quota_reset (monthly@day, tz=UTC+8).
    for (user_id, user) in &users_v2 {
        validate_cycle_day_of_month(user.cycle_day_of_month_default).map_err(StoreError::Domain)?;
        out.users.insert(
            user_id.clone(),
            User {
                user_id: user.user_id.clone(),
                display_name: user.display_name.clone(),
                subscription_token: user.subscription_token.clone(),
                quota_reset: UserQuotaReset::Monthly {
                    day_of_month: user.cycle_day_of_month_default,
                    tz_offset_minutes: 480,
                },
            },
        );
    }

    // Preserve explicit user-node quotas (quota only; reset source migrated below).
    for (user_id, nodes) in user_node_quotas {
        for (node_id, quota_limit_bytes) in nodes {
            out.user_node_quotas
                .entry(user_id.clone())
                .or_default()
                .insert(
                    node_id,
                    UserNodeQuotaConfig {
                        quota_limit_bytes: Some(quota_limit_bytes),
                        // In v2 this field did not exist; seed with the default and allow it to be
                        // overridden based on grants later in this migration.
                        quota_reset_source: QuotaResetSource::User,
                    },
                );
        }
    }

    let mut group_key_to_name = std::collections::BTreeMap::<String, String>::new();
    let mut used_group_names = std::collections::BTreeSet::<String>::new();
    let mut seen_pairs = std::collections::BTreeSet::<(String, String)>::new();
    let mut node_day_by_node_id = std::collections::BTreeMap::<String, u8>::new();

    for (_grant_id, grant) in grants {
        let endpoint =
            out.endpoints
                .get(&grant.endpoint_id)
                .ok_or_else(|| StoreError::Migration {
                    message: format!("missing endpoint for grant_id={}", grant.grant_id),
                })?;
        let node_id = endpoint.node_id.clone();

        let (effective_source, effective_day) = match grant.cycle_policy {
            CyclePolicyV2::InheritUser => {
                let user = users_v2
                    .get(&grant.user_id)
                    .ok_or_else(|| StoreError::Migration {
                        message: format!("missing user for grant_id={}", grant.grant_id),
                    })?;
                let source = match user.cycle_policy_default {
                    CyclePolicyDefaultV2::ByUser => QuotaResetSource::User,
                    CyclePolicyDefaultV2::ByNode => QuotaResetSource::Node,
                };
                (source, user.cycle_day_of_month_default)
            }
            CyclePolicyV2::ByUser => (
                QuotaResetSource::User,
                grant
                    .cycle_day_of_month
                    .ok_or_else(|| StoreError::Migration {
                        message: format!(
                            "missing cycle_day_of_month for grant_id={} (cycle_policy=by_user)",
                            grant.grant_id
                        ),
                    })?,
            ),
            CyclePolicyV2::ByNode => (
                QuotaResetSource::Node,
                grant
                    .cycle_day_of_month
                    .ok_or_else(|| StoreError::Migration {
                        message: format!(
                            "missing cycle_day_of_month for grant_id={} (cycle_policy=by_node)",
                            grant.grant_id
                        ),
                    })?,
            ),
        };

        validate_cycle_day_of_month(effective_day).map_err(StoreError::Domain)?;

        if effective_source == QuotaResetSource::Node {
            match node_day_by_node_id.get(&node_id) {
                Some(existing) if *existing != effective_day => {
                    return Err(StoreError::Migration {
                        message: format!(
                            "conflicting node day_of_month for node_id={node_id}: {existing} vs {effective_day}"
                        ),
                    });
                }
                None => {
                    node_day_by_node_id.insert(node_id.clone(), effective_day);
                }
                _ => {}
            }
        }

        let pair = (grant.user_id.clone(), grant.endpoint_id.clone());
        if !seen_pairs.insert(pair.clone()) {
            return Err(StoreError::Migration {
                message: format!(
                    "duplicate (user_id, endpoint_id) detected: user_id={} endpoint_id={}",
                    pair.0, pair.1
                ),
            });
        }

        let cfg_effective_source = effective_source.clone();
        let cfg = out
            .user_node_quotas
            .entry(grant.user_id.clone())
            .or_default()
            .entry(node_id.clone())
            .or_insert(UserNodeQuotaConfig {
                quota_limit_bytes: None,
                quota_reset_source: cfg_effective_source,
            });
        // If this config came from the legacy v2 `user_node_quotas` map, its `quota_reset_source`
        // was seeded with the default and should be replaced with the effective source derived
        // from grants.
        if cfg.quota_limit_bytes.is_some() && cfg.quota_reset_source == QuotaResetSource::User {
            cfg.quota_reset_source = effective_source.clone();
        }
        if cfg.quota_reset_source != effective_source {
            return Err(StoreError::Migration {
                message: format!(
                    "conflicting quota_reset_source for user_id={} node_id={}: {:?} vs {:?}",
                    grant.user_id, node_id, cfg.quota_reset_source, effective_source
                ),
            });
        }

        let group_key = grant
            .group_name
            .as_deref()
            .map(|raw| format!("raw:{raw}"))
            .unwrap_or_else(|| format!("legacy-user:{}", grant.user_id));
        let group_name = group_key_to_name.entry(group_key).or_insert_with(|| {
            let base = grant
                .group_name
                .as_deref()
                .map(sanitize_group_name_fragment)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| make_legacy_group_name(&grant.user_id));
            let mut candidate = base;
            if candidate.len() > 64 {
                candidate.truncate(64);
            }
            if candidate.is_empty() {
                candidate = make_legacy_group_name(&grant.user_id);
            }
            if validate_group_name(&candidate).is_err() {
                candidate = make_legacy_group_name(&grant.user_id);
            }
            let base_candidate = candidate.clone();
            if used_group_names.insert(candidate.clone()) {
                return candidate;
            }
            let mut i = 2u32;
            loop {
                let suffix = format!("-{i}");
                let mut with_suffix = base_candidate.clone();
                if with_suffix.len() + suffix.len() > 64 {
                    with_suffix.truncate(64 - suffix.len());
                }
                with_suffix.push_str(&suffix);
                if used_group_names.insert(with_suffix.clone()) {
                    return with_suffix;
                }
                i += 1;
            }
        });

        out.grants.insert(
            grant.grant_id.clone(),
            Grant {
                grant_id: grant.grant_id,
                group_name: group_name.clone(),
                user_id: grant.user_id,
                endpoint_id: grant.endpoint_id,
                enabled: grant.enabled,
                quota_limit_bytes: grant.quota_limit_bytes,
                note: grant.note,
                credentials: grant.credentials,
            },
        );
    }

    for (node_id, node) in out.nodes.iter_mut() {
        let day = node_day_by_node_id.get(node_id).copied().unwrap_or(1);
        node.quota_reset = NodeQuotaReset::Monthly {
            day_of_month: day,
            tz_offset_minutes: None,
        };
    }

    Ok(out)
}

fn migrate_v3_to_v4(mut input: PersistedState) -> Result<PersistedState, StoreError> {
    if input.schema_version != 3 {
        return Err(StoreError::Migration {
            message: format!(
                "unexpected schema version for v3->v4 migration: {}",
                input.schema_version
            ),
        });
    }
    input.schema_version = SCHEMA_VERSION_V4;
    Ok(input)
}

fn default_seed_reality_domains() -> Vec<RealityDomain> {
    // Deterministic IDs: avoid cluster divergence when seeding during migrations.
    vec![
        RealityDomain {
            domain_id: "seed_public_sn_files_1drv_com".to_string(),
            server_name: "public.sn.files.1drv.com".to_string(),
            disabled_node_ids: BTreeSet::new(),
        },
        RealityDomain {
            domain_id: "seed_public_bn_files_1drv_com".to_string(),
            server_name: "public.bn.files.1drv.com".to_string(),
            disabled_node_ids: BTreeSet::new(),
        },
        RealityDomain {
            domain_id: "seed_oneclient_sfx_ms".to_string(),
            server_name: "oneclient.sfx.ms".to_string(),
            disabled_node_ids: BTreeSet::new(),
        },
    ]
}

fn migrate_v4_to_v5(mut input: PersistedState) -> Result<PersistedState, StoreError> {
    if input.schema_version != SCHEMA_VERSION_V4 {
        return Err(StoreError::Migration {
            message: format!(
                "unexpected schema version for v4->v5 migration: {}",
                input.schema_version
            ),
        });
    }
    input.schema_version = SCHEMA_VERSION;
    if input.reality_domains.is_empty() {
        input.reality_domains = default_seed_reality_domains();
    }
    Ok(input)
}

#[cfg(test)]
mod migrate_tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn migrate_v2_like_to_v3_overrides_seeded_quota_reset_source_from_grants() {
        let node_id = "node_1".to_string();
        let endpoint_id = "endpoint_1".to_string();
        let user_id = "user_1".to_string();

        let mut nodes = BTreeMap::new();
        nodes.insert(
            node_id.clone(),
            Node {
                node_id: node_id.clone(),
                node_name: "node-1".to_string(),
                access_host: "".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_reset: NodeQuotaReset::default(),
            },
        );

        let mut endpoints = BTreeMap::new();
        endpoints.insert(
            endpoint_id.clone(),
            Endpoint {
                endpoint_id: endpoint_id.clone(),
                node_id: node_id.clone(),
                tag: "test".to_string(),
                kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                port: 12345,
                meta: serde_json::json!({}),
            },
        );

        let mut users = BTreeMap::new();
        users.insert(
            user_id.clone(),
            UserV2 {
                user_id: user_id.clone(),
                display_name: "alice".to_string(),
                subscription_token: "token".to_string(),
                cycle_policy_default: CyclePolicyDefaultV2::ByUser,
                cycle_day_of_month_default: 1,
            },
        );

        let mut grants = BTreeMap::new();
        grants.insert(
            "grant_1".to_string(),
            GrantV2 {
                grant_id: "grant_1".to_string(),
                user_id: user_id.clone(),
                endpoint_id: endpoint_id.clone(),
                group_name: None,
                enabled: true,
                quota_limit_bytes: 123,
                cycle_policy: CyclePolicyV2::ByNode,
                cycle_day_of_month: Some(1),
                note: None,
                credentials: GrantCredentials {
                    vless: None,
                    ss2022: None,
                },
            },
        );

        let mut user_node_quotas = BTreeMap::new();
        user_node_quotas
            .entry(user_id.clone())
            .or_insert_with(BTreeMap::new)
            .insert(node_id.clone(), 456);

        let v2 = PersistedStateV2Like {
            schema_version: 2,
            nodes,
            endpoints,
            users,
            grants,
            user_node_quotas,
        };

        let v3 = migrate_v2_like_to_v3(v2).expect("migration should succeed");
        let cfg = v3
            .user_node_quotas
            .get(&user_id)
            .and_then(|m| m.get(&node_id))
            .expect("user node quota cfg should exist");

        assert_eq!(cfg.quota_limit_bytes, Some(456));
        assert_eq!(cfg.quota_reset_source, QuotaResetSource::Node);
    }

    #[test]
    fn migrate_v4_to_v5_seeds_reality_domains_when_empty() {
        let mut v4 = PersistedState::empty();
        v4.schema_version = SCHEMA_VERSION_V4;
        v4.reality_domains = Vec::new();

        let v5 = migrate_v4_to_v5(v4).expect("migration should succeed");
        assert_eq!(v5.schema_version, SCHEMA_VERSION);
        assert_eq!(v5.reality_domains, default_seed_reality_domains());
    }

    #[test]
    fn migrate_v4_to_v5_does_not_override_existing_reality_domains() {
        let mut v4 = PersistedState::empty();
        v4.schema_version = SCHEMA_VERSION_V4;
        v4.reality_domains = vec![RealityDomain {
            domain_id: "custom_1".to_string(),
            server_name: "example.com".to_string(),
            disabled_node_ids: BTreeSet::new(),
        }];

        let v5 = migrate_v4_to_v5(v4).expect("migration should succeed");
        assert_eq!(v5.schema_version, SCHEMA_VERSION);
        assert_eq!(v5.reality_domains.len(), 1);
        assert_eq!(v5.reality_domains[0].domain_id, "custom_1");
        assert_eq!(v5.reality_domains[0].server_name, "example.com");
    }
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(Some(value))
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GrantEnabledSource {
    #[default]
    Manual,
    Quota,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesiredStateCommand {
    UpsertNode {
        node: Node,
    },
    DeleteNode {
        node_id: String,
    },
    UpsertEndpoint {
        endpoint: Endpoint,
    },
    DeleteEndpoint {
        endpoint_id: String,
    },
    CreateRealityDomain {
        domain: RealityDomain,
    },
    PatchRealityDomain {
        domain_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        server_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        disabled_node_ids: Option<BTreeSet<String>>,
    },
    DeleteRealityDomain {
        domain_id: String,
    },
    ReorderRealityDomains {
        domain_ids: Vec<String>,
    },
    UpsertUser {
        user: User,
    },
    DeleteUser {
        user_id: String,
    },
    ResetUserSubscriptionToken {
        user_id: String,
        subscription_token: String,
    },
    SetUserNodeQuota {
        user_id: String,
        node_id: String,
        quota_limit_bytes: u64,
        #[serde(default)]
        quota_reset_source: QuotaResetSource,
    },
    UpsertGrant {
        grant: Grant,
    },
    DeleteGrant {
        grant_id: String,
    },
    CreateGrantGroup {
        group_name: String,
        grants: Vec<Grant>,
    },
    ReplaceGrantGroup {
        group_name: String,
        #[serde(default, deserialize_with = "deserialize_optional_string")]
        rename_to: Option<Option<String>>,
        grants: Vec<Grant>,
    },
    DeleteGrantGroup {
        group_name: String,
    },
    SetGrantEnabled {
        grant_id: String,
        enabled: bool,
        #[serde(default)]
        source: GrantEnabledSource,
    },
    AppendEndpointProbeSamples {
        /// Hour bucket key like `2026-02-07T12:00:00Z`.
        hour: String,
        from_node_id: String,
        samples: Vec<EndpointProbeAppendSample>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesiredStateApplyResult {
    Applied,
    NodeDeleted {
        deleted: bool,
    },
    EndpointDeleted {
        deleted: bool,
    },
    UserDeleted {
        deleted: bool,
    },
    UserTokenReset {
        applied: bool,
    },
    UserNodeQuotaSet {
        quota: UserNodeQuota,
    },
    GrantDeleted {
        deleted: bool,
    },
    GrantGroupCreated {
        created: usize,
    },
    GrantGroupReplaced {
        group_name: String,
        created: usize,
        updated: usize,
        deleted: usize,
    },
    GrantGroupDeleted {
        deleted: usize,
    },
    GrantEnabledSet {
        grant: Option<Grant>,
        changed: bool,
    },
}

fn validate_user_quota_reset(reset: &UserQuotaReset) -> Result<(), DomainError> {
    match reset {
        UserQuotaReset::Unlimited { tz_offset_minutes } => {
            validate_tz_offset_minutes(*tz_offset_minutes)?;
        }
        UserQuotaReset::Monthly {
            day_of_month,
            tz_offset_minutes,
        } => {
            validate_cycle_day_of_month(*day_of_month)?;
            validate_tz_offset_minutes(*tz_offset_minutes)?;
        }
    }
    Ok(())
}

fn validate_node_quota_reset(reset: &NodeQuotaReset) -> Result<(), DomainError> {
    match reset {
        NodeQuotaReset::Unlimited { tz_offset_minutes } => {
            if let Some(tz_offset_minutes) = tz_offset_minutes {
                validate_tz_offset_minutes(*tz_offset_minutes)?;
            }
        }
        NodeQuotaReset::Monthly {
            day_of_month,
            tz_offset_minutes,
        } => {
            validate_cycle_day_of_month(*day_of_month)?;
            if let Some(tz_offset_minutes) = tz_offset_minutes {
                validate_tz_offset_minutes(*tz_offset_minutes)?;
            }
        }
    }
    Ok(())
}

fn normalize_reality_server_names(input: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::<String>::new();
    for raw in input {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Domain names are case-insensitive; dedupe by a canonical lowercase key.
        let key = trimmed.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(trimmed.to_string());
    }
    out
}

fn derive_global_reality_server_names(domains: &[RealityDomain], node_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::<String>::new();
    for domain in domains.iter() {
        if domain.disabled_node_ids.contains(node_id) {
            continue;
        }
        let trimmed = domain.server_name.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(trimmed.to_string());
    }
    out
}

fn build_global_vless_meta_updates(
    endpoints: &BTreeMap<String, Endpoint>,
    domains: &[RealityDomain],
) -> Result<BTreeMap<String, serde_json::Value>, StoreError> {
    let mut out = BTreeMap::<String, serde_json::Value>::new();
    for (endpoint_id, endpoint) in endpoints.iter() {
        if endpoint.kind != EndpointKind::VlessRealityVisionTcp {
            continue;
        }
        let mut meta: VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta.clone())?;
        if meta.reality.server_names_source != RealityServerNamesSource::Global {
            continue;
        }

        let derived = derive_global_reality_server_names(domains, &endpoint.node_id);
        if derived.is_empty() {
            return Err(DomainError::RealityDomainsWouldBreakEndpoint {
                endpoint_id: endpoint_id.clone(),
                node_id: endpoint.node_id.clone(),
            }
            .into());
        }

        meta.reality.server_names = derived;
        meta.reality.dest = format!("{}:443", meta.reality.server_names[0].trim());
        out.insert(endpoint_id.clone(), serde_json::to_value(meta)?);
    }
    Ok(out)
}

fn apply_vless_meta_updates(
    endpoints: &mut BTreeMap<String, Endpoint>,
    updates: BTreeMap<String, serde_json::Value>,
) -> Result<(), StoreError> {
    for (endpoint_id, meta) in updates.into_iter() {
        if let Some(endpoint) = endpoints.get_mut(&endpoint_id) {
            endpoint.meta = meta;
        }
    }
    Ok(())
}

impl DesiredStateCommand {
    pub fn apply(&self, state: &mut PersistedState) -> Result<DesiredStateApplyResult, StoreError> {
        match self {
            Self::UpsertNode { node } => {
                validate_node_quota_reset(&node.quota_reset)?;
                state.nodes.insert(node.node_id.clone(), node.clone());
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::DeleteNode { node_id } => {
                if !state.nodes.contains_key(node_id) {
                    return Ok(DesiredStateApplyResult::NodeDeleted { deleted: false });
                }

                if let Some(endpoint) = state
                    .endpoints
                    .values()
                    .find(|endpoint| endpoint.node_id == *node_id)
                {
                    return Err(crate::domain::DomainError::NodeInUse {
                        node_id: node_id.clone(),
                        endpoint_id: endpoint.endpoint_id.clone(),
                    }
                    .into());
                }

                state.nodes.remove(node_id);
                for domain in state.reality_domains.iter_mut() {
                    domain.disabled_node_ids.remove(node_id);
                }

                // A node-scoped quota config becomes meaningless once the node is removed.
                for (_user_id, nodes) in state.user_node_quotas.iter_mut() {
                    nodes.remove(node_id);
                }
                state
                    .user_node_quotas
                    .retain(|_user_id, nodes| !nodes.is_empty());

                // Cleanup endpoint probe samples for the removed node.
                for (_endpoint_id, history) in state.endpoint_probe_history.iter_mut() {
                    for (_hour, bucket) in history.hours.iter_mut() {
                        bucket.by_node.remove(node_id);
                    }
                    history
                        .hours
                        .retain(|_hour, bucket| !bucket.by_node.is_empty());
                }
                state
                    .endpoint_probe_history
                    .retain(|_endpoint_id, history| !history.hours.is_empty());

                Ok(DesiredStateApplyResult::NodeDeleted { deleted: true })
            }
            Self::UpsertEndpoint { endpoint } => {
                validate_port(endpoint.port)?;

                let mut endpoint = endpoint.clone();
                if endpoint.kind == EndpointKind::VlessRealityVisionTcp {
                    let mut meta: VlessRealityVisionTcpEndpointMeta =
                        serde_json::from_value(endpoint.meta.clone())?;

                    let server_names = match meta.reality.server_names_source {
                        RealityServerNamesSource::Manual => {
                            let normalized =
                                normalize_reality_server_names(&meta.reality.server_names);
                            if normalized.is_empty() {
                                return Err(DomainError::VlessRealityServerNamesEmpty {
                                    endpoint_id: endpoint.endpoint_id.clone(),
                                }
                                .into());
                            }
                            for name in normalized.iter() {
                                validate_reality_server_name(name).map_err(|reason| {
                                    DomainError::InvalidRealityServerName {
                                        server_name: name.clone(),
                                        reason: reason.to_string(),
                                    }
                                })?;
                            }
                            normalized
                        }
                        RealityServerNamesSource::Global => {
                            let derived = derive_global_reality_server_names(
                                &state.reality_domains,
                                &endpoint.node_id,
                            );
                            if derived.is_empty() {
                                return Err(DomainError::RealityDomainsWouldBreakEndpoint {
                                    endpoint_id: endpoint.endpoint_id.clone(),
                                    node_id: endpoint.node_id.clone(),
                                }
                                .into());
                            }
                            for name in derived.iter() {
                                validate_reality_server_name(name).map_err(|reason| {
                                    DomainError::InvalidRealityServerName {
                                        server_name: name.clone(),
                                        reason: reason.to_string(),
                                    }
                                })?;
                            }
                            derived
                        }
                    };

                    meta.reality.server_names = server_names;
                    meta.reality.dest = format!("{}:443", meta.reality.server_names[0].trim());
                    endpoint.meta = serde_json::to_value(meta)?;
                }

                state
                    .endpoints
                    .insert(endpoint.endpoint_id.clone(), endpoint);
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::DeleteEndpoint { endpoint_id } => {
                let deleted = state.endpoints.remove(endpoint_id).is_some();
                state.endpoint_probe_history.remove(endpoint_id);
                Ok(DesiredStateApplyResult::EndpointDeleted { deleted })
            }
            Self::CreateRealityDomain { domain } => {
                let mut domain = domain.clone();
                domain.server_name = domain.server_name.trim().to_string();
                if let Err(reason) = validate_reality_server_name(domain.server_name.as_str()) {
                    return Err(DomainError::InvalidRealityServerName {
                        server_name: domain.server_name,
                        reason: reason.to_string(),
                    }
                    .into());
                }

                // Uniqueness (case-insensitive).
                let key = domain.server_name.to_ascii_lowercase();
                if state
                    .reality_domains
                    .iter()
                    .any(|d| d.server_name.to_ascii_lowercase() == key)
                {
                    return Err(DomainError::RealityDomainNameConflict {
                        server_name: domain.server_name,
                    }
                    .into());
                }

                // Node IDs must exist.
                for node_id in domain.disabled_node_ids.iter() {
                    if !state.nodes.contains_key(node_id) {
                        return Err(DomainError::MissingNode {
                            node_id: node_id.clone(),
                        }
                        .into());
                    }
                }

                let mut next_domains = state.reality_domains.clone();
                next_domains.push(domain);
                let updates = build_global_vless_meta_updates(&state.endpoints, &next_domains)?;

                state.reality_domains = next_domains;
                apply_vless_meta_updates(&mut state.endpoints, updates)?;

                Ok(DesiredStateApplyResult::Applied)
            }
            Self::PatchRealityDomain {
                domain_id,
                server_name,
                disabled_node_ids,
            } => {
                let Some(existing_idx) = state
                    .reality_domains
                    .iter()
                    .position(|d| d.domain_id == *domain_id)
                else {
                    return Err(DomainError::RealityDomainNotFound {
                        domain_id: domain_id.clone(),
                    }
                    .into());
                };

                let mut next_domains = state.reality_domains.clone();
                let mut next = next_domains
                    .get(existing_idx)
                    .cloned()
                    .expect("index checked above");

                if let Some(server_name) = server_name.as_ref() {
                    let trimmed = server_name.trim();
                    if let Err(reason) = validate_reality_server_name(trimmed) {
                        return Err(DomainError::InvalidRealityServerName {
                            server_name: trimmed.to_string(),
                            reason: reason.to_string(),
                        }
                        .into());
                    }

                    let key = trimmed.to_ascii_lowercase();
                    if next_domains.iter().any(|d| {
                        d.domain_id != next.domain_id && d.server_name.to_ascii_lowercase() == key
                    }) {
                        return Err(DomainError::RealityDomainNameConflict {
                            server_name: trimmed.to_string(),
                        }
                        .into());
                    }

                    next.server_name = trimmed.to_string();
                }

                if let Some(disabled) = disabled_node_ids.as_ref() {
                    for node_id in disabled.iter() {
                        if !state.nodes.contains_key(node_id) {
                            return Err(DomainError::MissingNode {
                                node_id: node_id.clone(),
                            }
                            .into());
                        }
                    }
                    next.disabled_node_ids = disabled.clone();
                }

                next_domains[existing_idx] = next;
                let updates = build_global_vless_meta_updates(&state.endpoints, &next_domains)?;

                state.reality_domains = next_domains;
                apply_vless_meta_updates(&mut state.endpoints, updates)?;

                Ok(DesiredStateApplyResult::Applied)
            }
            Self::DeleteRealityDomain { domain_id } => {
                if !state
                    .reality_domains
                    .iter()
                    .any(|d| d.domain_id == *domain_id)
                {
                    return Err(DomainError::RealityDomainNotFound {
                        domain_id: domain_id.clone(),
                    }
                    .into());
                }

                let mut next_domains: Vec<RealityDomain> = state
                    .reality_domains
                    .iter()
                    .filter(|d| d.domain_id != *domain_id)
                    .cloned()
                    .collect();

                // Ensure we do not end up with duplicate IDs (should be impossible) and keep
                // deterministic seed behavior.
                if next_domains.len() == state.reality_domains.len() {
                    return Err(DomainError::RealityDomainNotFound {
                        domain_id: domain_id.clone(),
                    }
                    .into());
                }

                let updates = build_global_vless_meta_updates(&state.endpoints, &next_domains)?;

                state.reality_domains = std::mem::take(&mut next_domains);
                apply_vless_meta_updates(&mut state.endpoints, updates)?;

                Ok(DesiredStateApplyResult::Applied)
            }
            Self::ReorderRealityDomains { domain_ids } => {
                if domain_ids.len() != state.reality_domains.len() {
                    return Err(DomainError::RealityDomainsReorderInvalid {
                        reason: format!(
                            "length mismatch: expected {} got {}",
                            state.reality_domains.len(),
                            domain_ids.len()
                        ),
                    }
                    .into());
                }

                let mut seen = BTreeSet::new();
                for id in domain_ids.iter() {
                    if !seen.insert(id.clone()) {
                        return Err(DomainError::RealityDomainsReorderInvalid {
                            reason: format!("duplicate domain_id: {id}"),
                        }
                        .into());
                    }
                }

                let mut by_id = std::collections::BTreeMap::<String, RealityDomain>::new();
                for d in state.reality_domains.iter() {
                    by_id.insert(d.domain_id.clone(), d.clone());
                }

                let mut next_domains = Vec::with_capacity(domain_ids.len());
                for id in domain_ids.iter() {
                    let Some(domain) = by_id.remove(id) else {
                        return Err(DomainError::RealityDomainsReorderInvalid {
                            reason: format!("unknown domain_id: {id}"),
                        }
                        .into());
                    };
                    next_domains.push(domain);
                }

                if !by_id.is_empty() {
                    return Err(DomainError::RealityDomainsReorderInvalid {
                        reason: "missing some domain_ids in reorder payload".to_string(),
                    }
                    .into());
                }

                let updates = build_global_vless_meta_updates(&state.endpoints, &next_domains)?;
                state.reality_domains = next_domains;
                apply_vless_meta_updates(&mut state.endpoints, updates)?;

                Ok(DesiredStateApplyResult::Applied)
            }
            Self::UpsertUser { user } => {
                validate_user_quota_reset(&user.quota_reset)?;
                state.users.insert(user.user_id.clone(), user.clone());
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::DeleteUser { user_id } => {
                let deleted = state.users.remove(user_id).is_some();
                Ok(DesiredStateApplyResult::UserDeleted { deleted })
            }
            Self::ResetUserSubscriptionToken {
                user_id,
                subscription_token,
            } => {
                let user = match state.users.get_mut(user_id) {
                    Some(user) => user,
                    None => return Ok(DesiredStateApplyResult::UserTokenReset { applied: false }),
                };
                user.subscription_token = subscription_token.clone();
                Ok(DesiredStateApplyResult::UserTokenReset { applied: true })
            }
            Self::SetUserNodeQuota {
                user_id,
                node_id,
                quota_limit_bytes,
                quota_reset_source,
            } => {
                if !state.users.contains_key(user_id) {
                    return Err(DomainError::MissingUser {
                        user_id: user_id.clone(),
                    }
                    .into());
                }
                if !state.nodes.contains_key(node_id) {
                    return Err(DomainError::MissingNode {
                        node_id: node_id.clone(),
                    }
                    .into());
                }

                state
                    .user_node_quotas
                    .entry(user_id.clone())
                    .or_default()
                    .insert(
                        node_id.clone(),
                        UserNodeQuotaConfig {
                            quota_limit_bytes: Some(*quota_limit_bytes),
                            quota_reset_source: quota_reset_source.clone(),
                        },
                    );

                // Best-effort: unify existing grants on that node to keep legacy API behavior consistent.
                for grant in state.grants.values_mut() {
                    if grant.user_id != *user_id {
                        continue;
                    }
                    let Some(endpoint) = state.endpoints.get(&grant.endpoint_id) else {
                        continue;
                    };
                    if endpoint.node_id == *node_id {
                        grant.quota_limit_bytes = *quota_limit_bytes;
                    }
                }

                Ok(DesiredStateApplyResult::UserNodeQuotaSet {
                    quota: UserNodeQuota {
                        user_id: user_id.clone(),
                        node_id: node_id.clone(),
                        quota_limit_bytes: *quota_limit_bytes,
                        quota_reset_source: quota_reset_source.clone(),
                    },
                })
            }
            Self::UpsertGrant { grant } => {
                if !state.users.contains_key(&grant.user_id) {
                    return Err(DomainError::MissingUser {
                        user_id: grant.user_id.clone(),
                    }
                    .into());
                }
                if !state.endpoints.contains_key(&grant.endpoint_id) {
                    return Err(DomainError::MissingEndpoint {
                        endpoint_id: grant.endpoint_id.clone(),
                    }
                    .into());
                }

                let mut grant = grant.clone();
                if grant.group_name.is_empty() || validate_group_name(&grant.group_name).is_err() {
                    // Backward-compat: older Raft logs allowed null/empty group_name.
                    // Fold them into a deterministic legacy group derived from user_id.
                    grant.group_name = make_legacy_group_name(&grant.user_id);
                }
                validate_group_name(&grant.group_name)?;
                if let Some(endpoint) = state.endpoints.get(&grant.endpoint_id)
                    && let Some(user_map) = state.user_node_quotas.get(&grant.user_id)
                    && let Some(cfg) = user_map.get(&endpoint.node_id)
                    && let Some(quota_limit_bytes) = cfg.quota_limit_bytes
                {
                    grant.quota_limit_bytes = quota_limit_bytes;
                }

                if state.grants.values().any(|g| {
                    g.grant_id != grant.grant_id
                        && g.user_id == grant.user_id
                        && g.endpoint_id == grant.endpoint_id
                }) {
                    return Err(DomainError::GrantPairConflict {
                        user_id: grant.user_id.clone(),
                        endpoint_id: grant.endpoint_id.clone(),
                    }
                    .into());
                }

                state.grants.insert(grant.grant_id.clone(), grant.clone());
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::DeleteGrant { grant_id } => {
                let deleted = state.grants.remove(grant_id).is_some();
                Ok(DesiredStateApplyResult::GrantDeleted { deleted })
            }
            Self::CreateGrantGroup { group_name, grants } => {
                validate_group_name(group_name)?;
                if grants.is_empty() {
                    return Err(DomainError::EmptyGrantGroup.into());
                }

                // group_name uniqueness.
                if state.grants.values().any(|g| g.group_name == *group_name) {
                    return Err(DomainError::GroupNameConflict {
                        group_name: group_name.clone(),
                    }
                    .into());
                }

                // members validation and global pair uniqueness.
                let mut seen_pairs = std::collections::BTreeSet::<(String, String)>::new();
                for grant in grants {
                    if !state.users.contains_key(&grant.user_id) {
                        return Err(DomainError::MissingUser {
                            user_id: grant.user_id.clone(),
                        }
                        .into());
                    }
                    if !state.endpoints.contains_key(&grant.endpoint_id) {
                        return Err(DomainError::MissingEndpoint {
                            endpoint_id: grant.endpoint_id.clone(),
                        }
                        .into());
                    }

                    if grant.group_name != *group_name {
                        return Err(DomainError::InvalidGroupName {
                            group_name: group_name.clone(),
                        }
                        .into());
                    }

                    let key = (grant.user_id.clone(), grant.endpoint_id.clone());
                    if !seen_pairs.insert(key.clone()) {
                        return Err(DomainError::DuplicateGrantGroupMember {
                            user_id: key.0,
                            endpoint_id: key.1,
                        }
                        .into());
                    }

                    // Global uniqueness: no existing (user_id, endpoint_id).
                    if state
                        .grants
                        .values()
                        .any(|g| g.user_id == key.0 && g.endpoint_id == key.1)
                    {
                        return Err(DomainError::GrantPairConflict {
                            user_id: key.0,
                            endpoint_id: key.1,
                        }
                        .into());
                    }

                    if state.grants.contains_key(&grant.grant_id) {
                        return Err(DomainError::InvalidGroupName {
                            group_name: group_name.clone(),
                        }
                        .into());
                    }
                }

                for grant in grants {
                    state.grants.insert(grant.grant_id.clone(), grant.clone());
                }

                Ok(DesiredStateApplyResult::GrantGroupCreated {
                    created: seen_pairs.len(),
                })
            }
            Self::ReplaceGrantGroup {
                group_name,
                rename_to,
                grants,
            } => {
                validate_group_name(group_name)?;
                if grants.is_empty() {
                    return Err(DomainError::EmptyGrantGroup.into());
                }

                let rename_to = rename_to.clone().flatten();
                if let Some(rename_to) = rename_to.as_deref() {
                    validate_group_name(rename_to)?;
                }

                let existing_ids: Vec<String> = state
                    .grants
                    .values()
                    .filter(|g| g.group_name == *group_name)
                    .map(|g| g.grant_id.clone())
                    .collect();
                if existing_ids.is_empty() {
                    return Err(DomainError::MissingGrantGroup {
                        group_name: group_name.clone(),
                    }
                    .into());
                }

                let target_group_name = rename_to.clone().unwrap_or_else(|| group_name.clone());
                if target_group_name != *group_name
                    && state
                        .grants
                        .values()
                        .any(|g| g.group_name == target_group_name)
                {
                    return Err(DomainError::GroupNameConflict {
                        group_name: target_group_name,
                    }
                    .into());
                }

                let mut desired_pairs = std::collections::BTreeSet::<(String, String)>::new();
                for grant in grants {
                    if grant.group_name != *group_name {
                        return Err(DomainError::InvalidGroupName {
                            group_name: group_name.clone(),
                        }
                        .into());
                    }
                    if !state.users.contains_key(&grant.user_id) {
                        return Err(DomainError::MissingUser {
                            user_id: grant.user_id.clone(),
                        }
                        .into());
                    }
                    if !state.endpoints.contains_key(&grant.endpoint_id) {
                        return Err(DomainError::MissingEndpoint {
                            endpoint_id: grant.endpoint_id.clone(),
                        }
                        .into());
                    }

                    let key = (grant.user_id.clone(), grant.endpoint_id.clone());
                    if !desired_pairs.insert(key.clone()) {
                        return Err(DomainError::DuplicateGrantGroupMember {
                            user_id: key.0,
                            endpoint_id: key.1,
                        }
                        .into());
                    }

                    // Global uniqueness: allow conflicts only with grants in this group.
                    if state.grants.values().any(|g| {
                        g.user_id == key.0 && g.endpoint_id == key.1 && g.group_name != *group_name
                    }) {
                        return Err(DomainError::GrantPairConflict {
                            user_id: key.0,
                            endpoint_id: key.1,
                        }
                        .into());
                    }
                }

                let mut created = 0usize;
                let mut updated = 0usize;
                let mut deleted = 0usize;

                let mut to_delete = Vec::new();
                for grant in state.grants.values() {
                    if grant.group_name != *group_name {
                        continue;
                    }
                    let key = (grant.user_id.clone(), grant.endpoint_id.clone());
                    if !desired_pairs.contains(&key) {
                        to_delete.push(grant.grant_id.clone());
                    }
                }
                for grant_id in &to_delete {
                    if state.grants.remove(grant_id).is_some() {
                        deleted += 1;
                    }
                }

                for mut grant in grants.clone() {
                    grant.group_name = target_group_name.clone();
                    if let Some(endpoint) = state.endpoints.get(&grant.endpoint_id)
                        && let Some(user_map) = state.user_node_quotas.get(&grant.user_id)
                        && let Some(cfg) = user_map.get(&endpoint.node_id)
                        && let Some(quota_limit_bytes) = cfg.quota_limit_bytes
                    {
                        grant.quota_limit_bytes = quota_limit_bytes;
                    }

                    if state.grants.contains_key(&grant.grant_id) {
                        state.grants.insert(grant.grant_id.clone(), grant);
                        updated += 1;
                    } else {
                        state.grants.insert(grant.grant_id.clone(), grant);
                        created += 1;
                    }
                }

                Ok(DesiredStateApplyResult::GrantGroupReplaced {
                    group_name: target_group_name,
                    created,
                    updated,
                    deleted,
                })
            }
            Self::DeleteGrantGroup { group_name } => {
                validate_group_name(group_name)?;
                let ids: Vec<String> = state
                    .grants
                    .values()
                    .filter(|g| g.group_name == *group_name)
                    .map(|g| g.grant_id.clone())
                    .collect();
                if ids.is_empty() {
                    return Err(DomainError::MissingGrantGroup {
                        group_name: group_name.clone(),
                    }
                    .into());
                }
                let mut deleted = 0usize;
                for grant_id in ids {
                    if state.grants.remove(&grant_id).is_some() {
                        deleted += 1;
                    }
                }
                Ok(DesiredStateApplyResult::GrantGroupDeleted { deleted })
            }
            Self::SetGrantEnabled {
                grant_id,
                enabled,
                source: _,
            } => {
                let grant = match state.grants.get_mut(grant_id) {
                    Some(grant) => grant,
                    None => {
                        return Ok(DesiredStateApplyResult::GrantEnabledSet {
                            grant: None,
                            changed: false,
                        });
                    }
                };

                if grant.enabled == *enabled {
                    return Ok(DesiredStateApplyResult::GrantEnabledSet {
                        grant: Some(grant.clone()),
                        changed: false,
                    });
                }

                grant.enabled = *enabled;
                Ok(DesiredStateApplyResult::GrantEnabledSet {
                    grant: Some(grant.clone()),
                    changed: true,
                })
            }
            Self::AppendEndpointProbeSamples {
                hour,
                from_node_id,
                samples,
            } => {
                for sample in samples {
                    if !state.endpoints.contains_key(&sample.endpoint_id) {
                        // Endpoints can be deleted concurrently with a probe run. Ignore samples
                        // for missing endpoints to keep probing resilient.
                        continue;
                    }

                    let history = state
                        .endpoint_probe_history
                        .entry(sample.endpoint_id.clone())
                        .or_default();
                    let bucket = history.hours.entry(hour.clone()).or_default();
                    bucket.by_node.insert(
                        from_node_id.clone(),
                        EndpointProbeNodeSample {
                            ok: sample.ok,
                            checked_at: sample.checked_at.clone(),
                            latency_ms: sample.latency_ms,
                            target_id: sample.target_id.clone(),
                            target_url: sample.target_url.clone(),
                            error: sample.error.clone(),
                            config_hash: sample.config_hash.clone(),
                        },
                    );

                    // Keep the latest 24 hour buckets to bound Raft state growth.
                    while history.hours.len() > 24 {
                        let Some(oldest) = history.hours.keys().next().cloned() else {
                            break;
                        };
                        history.hours.remove(&oldest);
                    }
                }

                Ok(DesiredStateApplyResult::Applied)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedUsage {
    pub schema_version: u32,
    #[serde(default)]
    pub grants: BTreeMap<String, GrantUsage>,
}

impl PersistedUsage {
    pub fn empty() -> Self {
        Self {
            schema_version: USAGE_SCHEMA_VERSION,
            grants: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrantUsage {
    pub cycle_start_at: String,
    pub cycle_end_at: String,
    pub used_bytes: u64,
    pub last_uplink_total: u64,
    pub last_downlink_total: u64,
    pub last_seen_at: String,
    #[serde(default)]
    pub quota_banned: bool,
    #[serde(default)]
    pub quota_banned_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub cycle_start_at: String,
    pub cycle_end_at: String,
    pub used_bytes: u64,
}

#[derive(Debug)]
pub struct JsonSnapshotStore {
    state_path: PathBuf,
    state: PersistedState,
    usage_path: PathBuf,
    usage: PersistedUsage,
}

impl JsonSnapshotStore {
    pub fn load_or_init(init: StoreInit) -> Result<Self, StoreError> {
        fs::create_dir_all(&init.data_dir)?;

        let state_path = init.data_dir.join("state.json");
        let (mut state, is_new_state, mut migrated) = if state_path.exists() {
            let bytes = fs::read(&state_path)?;
            let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
            let schema_version = raw
                .get("schema_version")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            match schema_version {
                5 => (serde_json::from_value(raw)?, false, false),
                4 => {
                    let v4: PersistedState = serde_json::from_value(raw)?;
                    let v5 = migrate_v4_to_v5(v4)?;
                    (v5, false, true)
                }
                3 => {
                    let v3: PersistedState = serde_json::from_value(raw)?;
                    let v4 = migrate_v3_to_v4(v3)?;
                    let v5 = migrate_v4_to_v5(v4)?;
                    (v5, false, true)
                }
                2 | 1 => {
                    let v2: PersistedStateV2Like = serde_json::from_value(raw)?;
                    let v4 = migrate_v2_like_to_v3(v2)?;
                    let v5 = migrate_v4_to_v5(v4)?;
                    (v5, false, true)
                }
                got => {
                    return Err(StoreError::SchemaVersionMismatch {
                        expected: SCHEMA_VERSION,
                        got,
                    });
                }
            }
        } else {
            let node_id = init.bootstrap_node_id.unwrap_or_else(new_ulid_string);
            let node = Node {
                node_id: node_id.clone(),
                node_name: init.bootstrap_node_name,
                access_host: init.bootstrap_access_host,
                api_base_url: init.bootstrap_api_base_url,
                quota_reset: NodeQuotaReset::default(),
            };

            let mut state = PersistedState::empty();
            state.nodes.insert(node_id, node);
            state.reality_domains = default_seed_reality_domains();
            (state, true, false)
        };

        if state.schema_version != SCHEMA_VERSION {
            return Err(StoreError::SchemaVersionMismatch {
                expected: SCHEMA_VERSION,
                got: state.schema_version,
            });
        }

        // Backward-compatible cleanup: `public_domain` used to exist in VLESS endpoint meta,
        // but it's a redundant xp-only field and is not used by the system.
        for endpoint in state.endpoints.values_mut() {
            if endpoint.kind == EndpointKind::VlessRealityVisionTcp
                && let Some(meta) = endpoint.meta.as_object_mut()
                && meta.remove("public_domain").is_some()
            {
                migrated = true;
            }
        }

        let usage_path = init.data_dir.join("usage.json");
        let usage = if usage_path.exists() {
            let bytes = fs::read(&usage_path)?;
            let usage: PersistedUsage = serde_json::from_slice(&bytes)?;
            if usage.schema_version != USAGE_SCHEMA_VERSION {
                return Err(StoreError::SchemaVersionMismatch {
                    expected: USAGE_SCHEMA_VERSION,
                    got: usage.schema_version,
                });
            }
            usage
        } else {
            PersistedUsage::empty()
        };

        let store = Self {
            state_path,
            state,
            usage_path,
            usage,
        };

        if is_new_state || migrated {
            store.save()?;
        }

        Ok(store)
    }

    pub fn state(&self) -> &PersistedState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut PersistedState {
        &mut self.state
    }

    pub fn save(&self) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec_pretty(&self.state)?;
        write_atomic(&self.state_path, &bytes)?;
        Ok(())
    }

    pub fn save_usage(&self) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec_pretty(&self.usage)?;
        write_atomic(&self.usage_path, &bytes)?;
        Ok(())
    }

    pub fn get_grant_usage(&self, grant_id: &str) -> Option<GrantUsage> {
        self.usage.grants.get(grant_id).cloned()
    }

    pub fn clear_grant_usage(&mut self, grant_id: &str) -> Result<(), StoreError> {
        if self.usage.grants.remove(grant_id).is_some() {
            self.save_usage()?;
        }
        Ok(())
    }

    pub fn set_quota_banned(
        &mut self,
        grant_id: &str,
        banned_at: String,
    ) -> Result<(), StoreError> {
        let entry = self
            .usage
            .grants
            .entry(grant_id.to_string())
            .or_insert_with(|| GrantUsage {
                cycle_start_at: banned_at.clone(),
                cycle_end_at: banned_at.clone(),
                used_bytes: 0,
                last_uplink_total: 0,
                last_downlink_total: 0,
                last_seen_at: banned_at.clone(),
                quota_banned: false,
                quota_banned_at: None,
            });

        entry.quota_banned = true;
        entry.quota_banned_at = Some(banned_at);
        self.save_usage()?;
        Ok(())
    }

    pub fn clear_quota_banned(&mut self, grant_id: &str) -> Result<(), StoreError> {
        if let Some(entry) = self.usage.grants.get_mut(grant_id) {
            entry.quota_banned = false;
            entry.quota_banned_at = None;
            self.save_usage()?;
        }
        Ok(())
    }

    pub fn apply_grant_usage_sample(
        &mut self,
        grant_id: &str,
        cycle_start_at: String,
        cycle_end_at: String,
        uplink_total: u64,
        downlink_total: u64,
        seen_at: String,
    ) -> Result<UsageSnapshot, StoreError> {
        let used_bytes = {
            let entry = self
                .usage
                .grants
                .entry(grant_id.to_string())
                .or_insert_with(|| GrantUsage {
                    cycle_start_at: cycle_start_at.clone(),
                    cycle_end_at: cycle_end_at.clone(),
                    used_bytes: uplink_total.saturating_add(downlink_total),
                    last_uplink_total: uplink_total,
                    last_downlink_total: downlink_total,
                    last_seen_at: seen_at.clone(),
                    quota_banned: false,
                    quota_banned_at: None,
                });

            if entry.cycle_start_at != cycle_start_at || entry.cycle_end_at != cycle_end_at {
                entry.cycle_start_at = cycle_start_at.clone();
                entry.cycle_end_at = cycle_end_at.clone();
                entry.used_bytes = 0;
                entry.last_uplink_total = uplink_total;
                entry.last_downlink_total = downlink_total;
                entry.last_seen_at = seen_at.clone();
                entry.used_bytes
            } else if uplink_total < entry.last_uplink_total
                || downlink_total < entry.last_downlink_total
            {
                // Counter reset / xray restart: don't subtract, just reset the baseline.
                entry.last_uplink_total = uplink_total;
                entry.last_downlink_total = downlink_total;
                entry.last_seen_at = seen_at.clone();
                entry.used_bytes
            } else {
                let delta_up = uplink_total - entry.last_uplink_total;
                let delta_down = downlink_total - entry.last_downlink_total;
                entry.used_bytes = entry
                    .used_bytes
                    .saturating_add(delta_up.saturating_add(delta_down));
                entry.last_uplink_total = uplink_total;
                entry.last_downlink_total = downlink_total;
                entry.last_seen_at = seen_at.clone();
                entry.used_bytes
            }
        };

        self.save_usage()?;
        Ok(UsageSnapshot {
            cycle_start_at,
            cycle_end_at,
            used_bytes,
        })
    }

    pub fn build_endpoint(
        &self,
        node_id: String,
        kind: EndpointKind,
        port: u16,
        meta: serde_json::Value,
    ) -> Result<Endpoint, StoreError> {
        let endpoint_id = new_ulid_string();
        let tag = endpoint_tag(&kind, &endpoint_id);

        let meta = build_endpoint_meta(&kind, meta)?;
        Ok(Endpoint {
            endpoint_id,
            node_id,
            tag,
            kind,
            port,
            meta,
        })
    }

    pub fn create_endpoint(
        &mut self,
        node_id: String,
        kind: EndpointKind,
        port: u16,
        meta: serde_json::Value,
    ) -> Result<Endpoint, StoreError> {
        let endpoint = self.build_endpoint(node_id, kind, port, meta)?;
        DesiredStateCommand::UpsertEndpoint {
            endpoint: endpoint.clone(),
        }
        .apply(&mut self.state)?;
        self.save()?;
        Ok(endpoint)
    }

    pub fn build_user(
        &self,
        display_name: String,
        quota_reset: Option<UserQuotaReset>,
    ) -> Result<User, StoreError> {
        let quota_reset = quota_reset.unwrap_or_default();
        validate_user_quota_reset(&quota_reset)?;

        let user_id = new_ulid_string();
        let subscription_token = format!("sub_{}", new_ulid_string());

        Ok(User {
            user_id,
            display_name,
            subscription_token,
            quota_reset,
        })
    }

    pub fn create_user(
        &mut self,
        display_name: String,
        quota_reset: Option<UserQuotaReset>,
    ) -> Result<User, StoreError> {
        let user = self.build_user(display_name, quota_reset)?;
        DesiredStateCommand::UpsertUser { user: user.clone() }.apply(&mut self.state)?;
        self.save()?;
        Ok(user)
    }

    pub fn build_grant(
        &self,
        group_name: String,
        user_id: String,
        endpoint_id: String,
        quota_limit_bytes: u64,
        enabled: bool,
        note: Option<String>,
    ) -> Result<Grant, StoreError> {
        validate_group_name(&group_name)?;
        if !self.state.users.contains_key(&user_id) {
            return Err(DomainError::MissingUser { user_id }.into());
        }
        let endpoint =
            self.state
                .endpoints
                .get(&endpoint_id)
                .ok_or_else(|| DomainError::MissingEndpoint {
                    endpoint_id: endpoint_id.clone(),
                })?;

        let quota_limit_bytes = self
            .state
            .user_node_quotas
            .get(&user_id)
            .and_then(|m| {
                m.get(&endpoint.node_id)
                    .and_then(|cfg| cfg.quota_limit_bytes)
            })
            .unwrap_or(quota_limit_bytes);

        let grant_id = new_ulid_string();
        let credentials = credentials_for_endpoint(endpoint, &grant_id)?;

        Ok(Grant {
            grant_id,
            group_name,
            user_id,
            endpoint_id,
            enabled,
            quota_limit_bytes,
            note,
            credentials,
        })
    }

    pub fn get_user_node_quota_limit_bytes(&self, user_id: &str, node_id: &str) -> Option<u64> {
        self.state
            .user_node_quotas
            .get(user_id)
            .and_then(|m| m.get(node_id).and_then(|cfg| cfg.quota_limit_bytes))
    }

    pub fn get_user_node_quota_reset_source(
        &self,
        user_id: &str,
        node_id: &str,
    ) -> Option<QuotaResetSource> {
        self.state
            .user_node_quotas
            .get(user_id)
            .and_then(|m| m.get(node_id).map(|cfg| cfg.quota_reset_source.clone()))
    }

    pub fn list_user_node_quotas(&self, user_id: &str) -> Result<Vec<UserNodeQuota>, StoreError> {
        if !self.state.users.contains_key(user_id) {
            return Err(DomainError::MissingUser {
                user_id: user_id.to_string(),
            }
            .into());
        }

        let mut out = Vec::new();
        if let Some(nodes) = self.state.user_node_quotas.get(user_id) {
            for (node_id, cfg) in nodes {
                let Some(quota_limit_bytes) = cfg.quota_limit_bytes else {
                    continue;
                };
                out.push(UserNodeQuota {
                    user_id: user_id.to_string(),
                    node_id: node_id.clone(),
                    quota_limit_bytes,
                    quota_reset_source: cfg.quota_reset_source.clone(),
                });
            }
        }
        Ok(out)
    }

    pub fn create_grant(
        &mut self,
        group_name: String,
        user_id: String,
        endpoint_id: String,
        quota_limit_bytes: u64,
        enabled: bool,
        note: Option<String>,
    ) -> Result<Grant, StoreError> {
        let grant = self.build_grant(
            group_name,
            user_id,
            endpoint_id,
            quota_limit_bytes,
            enabled,
            note,
        )?;
        DesiredStateCommand::UpsertGrant {
            grant: grant.clone(),
        }
        .apply(&mut self.state)?;
        self.save()?;
        Ok(grant)
    }

    pub fn list_nodes(&self) -> Vec<Node> {
        self.state.nodes.values().cloned().collect()
    }

    pub fn get_node(&self, node_id: &str) -> Option<Node> {
        self.state.nodes.get(node_id).cloned()
    }

    pub fn list_reality_domains(&self) -> Vec<RealityDomain> {
        self.state.reality_domains.clone()
    }

    pub fn get_reality_domain(&self, domain_id: &str) -> Option<RealityDomain> {
        self.state
            .reality_domains
            .iter()
            .find(|d| d.domain_id == domain_id)
            .cloned()
    }

    pub fn upsert_node(&mut self, node: Node) -> Result<Node, StoreError> {
        DesiredStateCommand::UpsertNode { node: node.clone() }.apply(&mut self.state)?;
        self.save()?;
        Ok(node)
    }

    pub fn list_endpoints(&self) -> Vec<Endpoint> {
        self.state.endpoints.values().cloned().collect()
    }

    pub fn get_endpoint(&self, endpoint_id: &str) -> Option<Endpoint> {
        self.state.endpoints.get(endpoint_id).cloned()
    }

    pub fn delete_endpoint(&mut self, endpoint_id: &str) -> Result<bool, StoreError> {
        let out = DesiredStateCommand::DeleteEndpoint {
            endpoint_id: endpoint_id.to_string(),
        }
        .apply(&mut self.state)?;
        let DesiredStateApplyResult::EndpointDeleted { deleted } = out else {
            unreachable!("delete endpoint must return EndpointDeleted");
        };
        if deleted {
            self.save()?;
        }
        Ok(deleted)
    }

    pub fn rotate_vless_reality_short_id(
        &mut self,
        endpoint_id: &str,
    ) -> Result<Option<RotateShortIdResult>, StoreError> {
        let mut rng = rand::rngs::OsRng;
        self.rotate_vless_reality_short_id_with_rng(endpoint_id, &mut rng)
    }

    pub fn build_rotate_vless_reality_short_id_command<R: rand::RngCore + rand::CryptoRng>(
        &self,
        endpoint_id: &str,
        rng: &mut R,
    ) -> Result<Option<(DesiredStateCommand, RotateShortIdResult)>, StoreError> {
        let mut endpoint = match self.state.endpoints.get(endpoint_id) {
            Some(endpoint) => endpoint.clone(),
            None => return Ok(None),
        };

        debug_assert_eq!(endpoint.kind, EndpointKind::VlessRealityVisionTcp);

        let mut meta: VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta.clone())?;

        let out = rotate_short_ids_in_place(&mut meta.short_ids, &mut meta.active_short_id, rng);

        endpoint.meta = serde_json::to_value(meta)?;

        Ok(Some((
            DesiredStateCommand::UpsertEndpoint { endpoint },
            out,
        )))
    }

    fn rotate_vless_reality_short_id_with_rng<R: rand::RngCore + rand::CryptoRng>(
        &mut self,
        endpoint_id: &str,
        rng: &mut R,
    ) -> Result<Option<RotateShortIdResult>, StoreError> {
        let Some((cmd, out)) =
            self.build_rotate_vless_reality_short_id_command(endpoint_id, rng)?
        else {
            return Ok(None);
        };

        cmd.apply(&mut self.state)?;
        self.save()?;
        Ok(Some(out))
    }

    pub fn list_users(&self) -> Vec<User> {
        self.state.users.values().cloned().collect()
    }

    pub fn get_user(&self, user_id: &str) -> Option<User> {
        self.state.users.get(user_id).cloned()
    }

    pub fn get_user_by_subscription_token(&self, subscription_token: &str) -> Option<User> {
        self.state
            .users
            .values()
            .find(|u| u.subscription_token == subscription_token)
            .cloned()
    }

    pub fn delete_user(&mut self, user_id: &str) -> Result<bool, StoreError> {
        let out = DesiredStateCommand::DeleteUser {
            user_id: user_id.to_string(),
        }
        .apply(&mut self.state)?;
        let DesiredStateApplyResult::UserDeleted { deleted } = out else {
            unreachable!("delete user must return UserDeleted");
        };
        if deleted {
            self.save()?;
        }
        Ok(deleted)
    }

    pub fn reset_user_token(&mut self, user_id: &str) -> Result<Option<String>, StoreError> {
        let subscription_token = format!("sub_{}", new_ulid_string());
        let out = DesiredStateCommand::ResetUserSubscriptionToken {
            user_id: user_id.to_string(),
            subscription_token: subscription_token.clone(),
        }
        .apply(&mut self.state)?;
        let DesiredStateApplyResult::UserTokenReset { applied } = out else {
            unreachable!("reset user token must return UserTokenReset");
        };
        if applied {
            self.save()?;
            Ok(Some(subscription_token))
        } else {
            Ok(None)
        }
    }

    pub fn list_grants(&self) -> Vec<Grant> {
        self.state.grants.values().cloned().collect()
    }

    pub fn get_grant(&self, grant_id: &str) -> Option<Grant> {
        self.state.grants.get(grant_id).cloned()
    }

    pub fn delete_grant(&mut self, grant_id: &str) -> Result<bool, StoreError> {
        let out = DesiredStateCommand::DeleteGrant {
            grant_id: grant_id.to_string(),
        }
        .apply(&mut self.state)?;
        let DesiredStateApplyResult::GrantDeleted { deleted } = out else {
            unreachable!("delete grant must return GrantDeleted");
        };
        if deleted {
            self.usage.grants.remove(grant_id);
            self.save()?;
            self.save_usage()?;
        }
        Ok(deleted)
    }

    pub fn set_grant_enabled(
        &mut self,
        grant_id: &str,
        enabled: bool,
        source: GrantEnabledSource,
    ) -> Result<Option<Grant>, StoreError> {
        let out = DesiredStateCommand::SetGrantEnabled {
            grant_id: grant_id.to_string(),
            enabled,
            source,
        }
        .apply(&mut self.state)?;
        let DesiredStateApplyResult::GrantEnabledSet { grant, changed } = out else {
            unreachable!("set grant enabled must return GrantEnabledSet");
        };
        if changed {
            self.save()?;
        }
        Ok(grant)
    }
}

#[derive(Debug, Deserialize)]
struct VlessRealityEndpointMetaInput {
    reality: crate::protocol::RealityConfig,
}

fn build_endpoint_meta(
    kind: &EndpointKind,
    meta_input: serde_json::Value,
) -> Result<serde_json::Value, StoreError> {
    let mut rng = rand::rngs::OsRng;

    match kind {
        EndpointKind::VlessRealityVisionTcp => {
            let input: VlessRealityEndpointMetaInput = serde_json::from_value(meta_input)?;
            let keypair = generate_reality_keypair(&mut rng);
            let short_id = generate_short_id_16hex(&mut rng);

            let meta = VlessRealityVisionTcpEndpointMeta {
                reality: input.reality,
                reality_keys: RealityKeys {
                    private_key: keypair.private_key,
                    public_key: keypair.public_key,
                },
                short_ids: vec![short_id.clone()],
                active_short_id: short_id,
            };

            Ok(serde_json::to_value(meta)?)
        }
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
            let server_psk_b64 = generate_ss2022_psk_b64(&mut rng);
            Ok(serde_json::to_value(Ss2022EndpointMeta {
                method: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                server_psk_b64,
            })?)
        }
    }
}

fn credentials_for_endpoint(
    endpoint: &Endpoint,
    grant_id: &str,
) -> Result<GrantCredentials, StoreError> {
    let mut rng = rand::rngs::OsRng;

    match endpoint.kind.clone() {
        EndpointKind::VlessRealityVisionTcp => Ok(GrantCredentials {
            vless: Some(VlessCredentials {
                uuid: Uuid::new_v4().to_string(),
                email: format!("grant:{grant_id}"),
            }),
            ss2022: None,
        }),
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
            let meta: Ss2022EndpointMeta = serde_json::from_value(endpoint.meta.clone())?;
            let user_psk_b64 = generate_ss2022_psk_b64(&mut rng);
            Ok(GrantCredentials {
                vless: None,
                ss2022: Some(Ss2022Credentials {
                    method: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
                    password: ss2022_password(&meta.server_psk_b64, &user_psk_b64),
                }),
            })
        }
    }
}

fn endpoint_tag(kind: &EndpointKind, endpoint_id: &str) -> String {
    let kind_short = match kind {
        EndpointKind::VlessRealityVisionTcp => "vless-vision",
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => "ss2022",
    };
    format!("{kind_short}-{endpoint_id}")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), io::Error> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp_path = dir.join(format!("{}.tmp", file_name.to_string_lossy()));
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        let _ = file.sync_all();
    }

    #[cfg(windows)]
    {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }

    fs::rename(tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;
    use rand::{SeedableRng as _, rngs::StdRng};
    use serde_json::json;

    use super::*;
    use crate::{
        domain::{
            DomainError, EndpointKind, Grant, GrantCredentials, NodeQuotaReset, UserQuotaReset,
            validate_cycle_day_of_month, validate_port,
        },
        id::is_ulid_string,
        protocol::{
            RealityConfig, RealityKeys, RealityServerNamesSource, VlessRealityVisionTcpEndpointMeta,
        },
    };

    fn test_init(tmp_dir: &Path) -> StoreInit {
        StoreInit {
            data_dir: tmp_dir.to_path_buf(),
            bootstrap_node_id: None,
            bootstrap_node_name: "node-1".to_string(),
            bootstrap_access_host: "".to_string(),
            bootstrap_api_base_url: "https://127.0.0.1:62416".to_string(),
        }
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
        assert_eq!(state.grants.len(), 0);

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
    fn set_grant_enabled_missing_source_defaults_manual() {
        let cmd: DesiredStateCommand = serde_json::from_value(json!({
            "type": "set_grant_enabled",
            "grant_id": "grant_1",
            "enabled": false
        }))
        .unwrap();

        match cmd {
            DesiredStateCommand::SetGrantEnabled { source, .. } => {
                assert_eq!(source, GrantEnabledSource::Manual);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn upsert_vless_endpoint_manual_enforces_dest_from_primary() {
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
        assert_eq!(meta.reality.dest, "b.example.com:443");
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
        let meta: VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta).unwrap();

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

        let grant_id = "grant_1";
        let usage_path = tmp.path().join("usage.json");
        let bytes = serde_json::to_vec_pretty(&json!({
            "schema_version": USAGE_SCHEMA_VERSION,
            "grants": {
                grant_id: {
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
        let usage = store.get_grant_usage(grant_id).unwrap();
        assert!(!usage.quota_banned);
        assert_eq!(usage.quota_banned_at, None);
    }

    #[test]
    fn set_and_clear_quota_banned_persists_and_survives_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let banned_at = "2025-12-18T00:00:00Z".to_string();
        let grant_id = "grant_1";

        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        store.set_quota_banned(grant_id, banned_at.clone()).unwrap();
        let usage = store.get_grant_usage(grant_id).unwrap();
        assert!(usage.quota_banned);
        assert_eq!(usage.quota_banned_at, Some(banned_at.clone()));

        store.clear_quota_banned(grant_id).unwrap();
        let usage = store.get_grant_usage(grant_id).unwrap();
        assert!(!usage.quota_banned);
        assert_eq!(usage.quota_banned_at, None);

        drop(store);

        let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        let usage = store.get_grant_usage(grant_id).unwrap();
        assert!(!usage.quota_banned);
        assert_eq!(usage.quota_banned_at, None);
    }

    #[test]
    fn apply_grant_usage_sample_keeps_quota_markers_on_cycle_change() {
        let tmp = tempfile::tempdir().unwrap();
        let grant_id = "grant_1";
        let banned_at = "2025-12-18T00:00:00Z".to_string();

        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        store
            .apply_grant_usage_sample(
                grant_id,
                "2025-12-01T00:00:00Z".to_string(),
                "2026-01-01T00:00:00Z".to_string(),
                10,
                20,
                "2025-12-18T00:00:00Z".to_string(),
            )
            .unwrap();
        store.set_quota_banned(grant_id, banned_at.clone()).unwrap();

        store
            .apply_grant_usage_sample(
                grant_id,
                "2026-01-01T00:00:00Z".to_string(),
                "2026-02-01T00:00:00Z".to_string(),
                0,
                0,
                "2026-01-01T00:00:00Z".to_string(),
            )
            .unwrap();

        let usage = store.get_grant_usage(grant_id).unwrap();
        assert!(usage.quota_banned);
        assert_eq!(usage.quota_banned_at, Some(banned_at));
    }

    #[test]
    fn deleting_grant_removes_usage_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let grant_id = "grant_1";

        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();

        store.state_mut().grants.insert(
            grant_id.to_string(),
            Grant {
                grant_id: grant_id.to_string(),
                group_name: "test-group".to_string(),
                user_id: "user_1".to_string(),
                endpoint_id: "endpoint_1".to_string(),
                enabled: true,
                quota_limit_bytes: 0,
                note: None,
                credentials: GrantCredentials {
                    vless: None,
                    ss2022: None,
                },
            },
        );

        store
            .set_quota_banned(grant_id, "2025-12-18T00:00:00Z".to_string())
            .unwrap();
        assert!(store.get_grant_usage(grant_id).is_some());

        assert!(store.delete_grant(grant_id).unwrap());
        assert!(store.get_grant_usage(grant_id).is_none());

        drop(store);

        let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        assert!(store.get_grant_usage(grant_id).is_none());
    }

    #[test]
    fn desired_state_apply_upsert_node_inserts_node() {
        let mut state = PersistedState::empty();
        let node = Node {
            node_id: "node_1".to_string(),
            node_name: "node-1".to_string(),
            access_host: "example.com".to_string(),
            api_base_url: "https://127.0.0.1:62416".to_string(),
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
    fn desired_state_apply_grant_create_update_delete_are_deterministic() {
        let mut state = PersistedState::empty();
        state.users.insert(
            "user_1".to_string(),
            User {
                user_id: "user_1".to_string(),
                display_name: "alice".to_string(),
                subscription_token: "sub_1".to_string(),
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

        let grant = Grant {
            grant_id: "grant_1".to_string(),
            group_name: "test-group".to_string(),
            user_id: "user_1".to_string(),
            endpoint_id: "endpoint_1".to_string(),
            enabled: true,
            quota_limit_bytes: 10,
            note: None,
            credentials: GrantCredentials {
                vless: None,
                ss2022: None,
            },
        };

        DesiredStateCommand::UpsertGrant {
            grant: grant.clone(),
        }
        .apply(&mut state)
        .unwrap();
        assert_eq!(state.grants.get(&grant.grant_id), Some(&grant));

        let out = DesiredStateCommand::SetGrantEnabled {
            grant_id: grant.grant_id.clone(),
            enabled: false,
            source: GrantEnabledSource::Manual,
        }
        .apply(&mut state)
        .unwrap();
        let DesiredStateApplyResult::GrantEnabledSet {
            grant: updated,
            changed,
        } = out
        else {
            panic!("expected GrantEnabledSet");
        };
        assert!(changed);
        let updated = updated.unwrap();
        assert!(!updated.enabled);
        assert_eq!(updated.quota_limit_bytes, 10);

        let out = DesiredStateCommand::DeleteGrant {
            grant_id: grant.grant_id.clone(),
        }
        .apply(&mut state)
        .unwrap();
        assert_eq!(out, DesiredStateApplyResult::GrantDeleted { deleted: true });
        assert!(!state.grants.contains_key(&grant.grant_id));
    }

    #[test]
    fn desired_state_apply_upsert_grant_fills_legacy_group_name_when_empty() {
        let mut state = PersistedState::empty();
        state.users.insert(
            "user_1".to_string(),
            User {
                user_id: "user_1".to_string(),
                display_name: "alice".to_string(),
                subscription_token: "sub_1".to_string(),
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

        let grant = Grant {
            grant_id: "grant_1".to_string(),
            group_name: "".to_string(),
            user_id: "user_1".to_string(),
            endpoint_id: "endpoint_1".to_string(),
            enabled: true,
            quota_limit_bytes: 10,
            note: None,
            credentials: GrantCredentials {
                vless: None,
                ss2022: None,
            },
        };

        DesiredStateCommand::UpsertGrant { grant }
            .apply(&mut state)
            .unwrap();
        let inserted = state.grants.get("grant_1").unwrap();
        assert_eq!(inserted.group_name, "legacy-user_1");
    }

    #[test]
    fn json_snapshot_store_create_update_delete_grant_flow_is_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
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
        let grant = store
            .create_grant(
                "test-group".to_string(),
                user.user_id.clone(),
                endpoint.endpoint_id.clone(),
                1024,
                true,
                None,
            )
            .unwrap();

        store
            .set_quota_banned(&grant.grant_id, "2025-12-18T00:00:00Z".to_string())
            .unwrap();
        assert!(store.get_grant_usage(&grant.grant_id).is_some());

        assert!(store.delete_grant(&grant.grant_id).unwrap());
        assert!(store.get_grant_usage(&grant.grant_id).is_none());

        drop(store);

        let store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        assert!(store.get_grant(&grant.grant_id).is_none());
        assert!(store.get_grant_usage(&grant.grant_id).is_none());
        assert!(store.get_user(&user.user_id).is_some());
        assert!(store.get_endpoint(&endpoint.endpoint_id).is_some());
    }
}
