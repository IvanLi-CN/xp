use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        DomainError, Endpoint, EndpointKind, Node, NodeQuotaReset, QuotaResetSource, RealityDomain,
        User, UserNodeQuota, UserPriorityTier, UserQuotaReset, validate_cycle_day_of_month,
        validate_port, validate_tz_offset_minutes,
    },
    id::new_ulid_string,
    protocol::{
        RealityKeys, RealityServerNamesSource, RotateShortIdResult,
        SS2022_METHOD_2022_BLAKE3_AES_128_GCM, Ss2022EndpointMeta,
        VlessRealityVisionTcpEndpointMeta, generate_reality_keypair, generate_short_id_16hex,
        generate_ss2022_psk_b64, rotate_short_ids_in_place, validate_reality_server_name,
    },
};

pub const SCHEMA_VERSION: u32 = 10;
const SCHEMA_VERSION_V9: u32 = 9;
const SCHEMA_VERSION_V8: u32 = 8;
const SCHEMA_VERSION_V7: u32 = 7;
const SCHEMA_VERSION_V6: u32 = 6;
const SCHEMA_VERSION_V5: u32 = 5;
const SCHEMA_VERSION_V4: u32 = 4;
pub const USAGE_SCHEMA_VERSION: u32 = 2;
const USAGE_SCHEMA_VERSION_V1: u32 = 1;

/// Migrate any historical state payload into the latest schema (v10).
///
/// This is used by Raft snapshot installation to support upgrades without requiring operators
/// to start an older binary for snapshot/purge first.
pub(crate) fn migrate_state_value_to_latest(
    raw: serde_json::Value,
) -> Result<PersistedState, StoreError> {
    let schema_version = raw
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let mut state = match schema_version {
        SCHEMA_VERSION => serde_json::from_value::<PersistedState>(raw)?,
        SCHEMA_VERSION_V9 | SCHEMA_VERSION_V8 | SCHEMA_VERSION_V7 | SCHEMA_VERSION_V6
        | SCHEMA_VERSION_V5 | SCHEMA_VERSION_V4 | 3 => {
            let mut legacy: PersistedStateV9Compat = serde_json::from_value(raw)?;
            let migrated = legacy.schema_version != schema_version;
            let _ = migrated;

            legacy = match schema_version {
                3 => migrate_v3_to_v4(legacy)?,
                _ => legacy,
            };
            legacy = if legacy.schema_version == SCHEMA_VERSION_V4 {
                migrate_v4_to_v5(legacy)?
            } else {
                legacy
            };
            legacy = if legacy.schema_version == SCHEMA_VERSION_V5 {
                migrate_v5_to_v6(legacy)?
            } else {
                legacy
            };
            legacy = if legacy.schema_version == SCHEMA_VERSION_V6 {
                migrate_v6_to_v7(legacy)?
            } else {
                legacy
            };
            legacy = if legacy.schema_version == SCHEMA_VERSION_V7 {
                migrate_v7_to_v8(legacy)?
            } else {
                legacy
            };

            let (v10, _mapping, _stats) = migrate_v9_compat_to_v10(legacy)?;
            v10
        }
        2 | 1 => {
            let v2: PersistedStateV2Like = serde_json::from_value(raw)?;
            let v4 = migrate_v2_like_to_v3(v2)?;
            let v5 = migrate_v4_to_v5(v4)?;
            let v6 = migrate_v5_to_v6(v5)?;
            let v7 = migrate_v6_to_v7(v6)?;
            let v8 = migrate_v7_to_v8(v7)?;

            let (v10, _mapping, _stats) = migrate_v9_compat_to_v10(v8)?;
            v10
        }
        got => {
            return Err(StoreError::SchemaVersionMismatch {
                expected: SCHEMA_VERSION,
                got,
            });
        }
    };

    if state.schema_version != SCHEMA_VERSION {
        return Err(StoreError::SchemaVersionMismatch {
            expected: SCHEMA_VERSION,
            got: state.schema_version,
        });
    }

    // Keep the same invariant cleanups as `load_or_init()` so snapshot installs don't
    // resurrect deprecated/invalid indexes.
    for endpoint in state.endpoints.values_mut() {
        if endpoint.kind == EndpointKind::VlessRealityVisionTcp
            && let Some(meta) = endpoint.meta.as_object_mut()
        {
            let _ = meta.remove("public_domain");
        }
    }

    state.user_global_weights =
        normalize_user_global_weights(&state, state.user_global_weights.clone());
    state.node_weight_policies =
        normalize_node_weight_policies(&state, state.node_weight_policies.clone());
    state.node_user_endpoint_memberships = normalize_node_user_endpoint_memberships(
        &state,
        state.node_user_endpoint_memberships.clone(),
    );
    state.node_user_endpoint_memberships = build_node_user_endpoint_memberships(&state);

    Ok(state)
}

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
    pub reality_domains: Vec<RealityDomain>,
    #[serde(default)]
    pub user_node_quotas: BTreeMap<String, BTreeMap<String, UserNodeQuotaConfig>>,
    #[serde(default)]
    pub user_node_weights: BTreeMap<String, BTreeMap<String, UserNodeWeightConfig>>,
    #[serde(default)]
    pub user_global_weights: BTreeMap<String, UserGlobalWeightConfig>,
    #[serde(default)]
    pub node_weight_policies: BTreeMap<String, NodeWeightPolicyConfig>,
    #[serde(default)]
    pub node_user_endpoint_memberships: BTreeSet<NodeUserEndpointMembership>,
    #[serde(default)]
    pub user_mihomo_profiles: BTreeMap<String, UserMihomoProfile>,
}

impl PersistedState {
    pub fn empty() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            nodes: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            endpoint_probe_history: BTreeMap::new(),
            users: BTreeMap::new(),
            reality_domains: Vec::new(),
            user_node_quotas: BTreeMap::new(),
            user_node_weights: BTreeMap::new(),
            user_global_weights: BTreeMap::new(),
            node_weight_policies: BTreeMap::new(),
            node_user_endpoint_memberships: BTreeSet::new(),
            user_mihomo_profiles: BTreeMap::new(),
        }
    }
}

/// Legacy persisted state (schema_version <= 9) that still includes `grants`.
///
/// This exists to support state.json + snapshot upgrades without requiring operators to
/// boot an old binary for cleanup.
///
/// TODO(remove-compat): remove once all clusters have upgraded to schema v10 and completed
/// at least one snapshot/purge cycle, then bump schema again (v11).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct PersistedStateV9Compat {
    pub schema_version: u32,
    #[serde(default)]
    pub nodes: BTreeMap<String, Node>,
    #[serde(default)]
    pub endpoints: BTreeMap<String, Endpoint>,
    #[serde(default)]
    pub endpoint_probe_history: BTreeMap<String, EndpointProbeHistory>,
    #[serde(default)]
    pub users: BTreeMap<String, User>,
    #[serde(default)]
    pub grants: BTreeMap<String, LegacyGrantCompat>,
    #[serde(default)]
    pub reality_domains: Vec<RealityDomain>,
    #[serde(default)]
    pub user_node_quotas: BTreeMap<String, BTreeMap<String, UserNodeQuotaConfig>>,
    #[serde(default)]
    pub user_node_weights: BTreeMap<String, BTreeMap<String, UserNodeWeightConfig>>,
    #[serde(default)]
    pub user_global_weights: BTreeMap<String, UserGlobalWeightConfig>,
    #[serde(default)]
    pub node_weight_policies: BTreeMap<String, NodeWeightPolicyConfig>,
    #[serde(default)]
    pub node_user_endpoint_memberships: BTreeSet<NodeUserEndpointMembership>,
}

impl PersistedStateV9Compat {
    fn empty_with_version(schema_version: u32) -> Self {
        Self {
            schema_version,
            nodes: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            endpoint_probe_history: BTreeMap::new(),
            users: BTreeMap::new(),
            grants: BTreeMap::new(),
            reality_domains: Vec::new(),
            user_node_quotas: BTreeMap::new(),
            user_node_weights: BTreeMap::new(),
            user_global_weights: BTreeMap::new(),
            node_weight_policies: BTreeMap::new(),
            node_user_endpoint_memberships: BTreeSet::new(),
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
    /// When true, this sample is intentionally skipped (reported but not tested).
    #[serde(default)]
    pub skipped: bool,
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
    /// When true, this sample is intentionally skipped (reported but not tested).
    #[serde(default)]
    pub skipped: bool,
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
pub struct UserNodeWeightConfig {
    pub weight: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserGlobalWeightConfig {
    pub weight: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeWeightPolicyConfig {
    #[serde(default = "default_true")]
    pub inherit_global: bool,
}

fn default_true() -> bool {
    true
}

impl Default for NodeWeightPolicyConfig {
    fn default() -> Self {
        Self {
            inherit_global: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NodeUserEndpointMembership {
    pub user_id: String,
    pub node_id: String,
    pub endpoint_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UserMihomoProfile {
    #[serde(default, alias = "template_yaml")]
    pub mixin_yaml: String,
    #[serde(default)]
    pub extra_proxies_yaml: String,
    #[serde(default)]
    pub extra_proxy_providers_yaml: String,
}

pub fn membership_key(user_id: &str, endpoint_id: &str) -> String {
    format!("{user_id}::{endpoint_id}")
}

pub fn membership_xray_email(user_id: &str, endpoint_id: &str) -> String {
    format!("m:{}", membership_key(user_id, endpoint_id))
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
    #[serde(default)]
    credentials: serde_json::Value,
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

fn migrate_v2_like_to_v3(
    input: PersistedStateV2Like,
) -> Result<PersistedStateV9Compat, StoreError> {
    let PersistedStateV2Like {
        schema_version: _,
        nodes,
        endpoints,
        users,
        grants,
        user_node_quotas,
    } = input;

    let users_v2 = users;

    let mut out = PersistedStateV9Compat::empty_with_version(SCHEMA_VERSION_V4);
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
                credential_epoch: 0,
                priority_tier: Default::default(),
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

        out.grants.insert(
            grant.grant_id.clone(),
            LegacyGrantCompat {
                grant_id: grant.grant_id,
                user_id: grant.user_id,
                endpoint_id: grant.endpoint_id,
                enabled: grant.enabled,
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

fn migrate_v3_to_v4(
    mut input: PersistedStateV9Compat,
) -> Result<PersistedStateV9Compat, StoreError> {
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

fn migrate_v4_to_v5(
    mut input: PersistedStateV9Compat,
) -> Result<PersistedStateV9Compat, StoreError> {
    if input.schema_version != SCHEMA_VERSION_V4 {
        return Err(StoreError::Migration {
            message: format!(
                "unexpected schema version for v4->v5 migration: {}",
                input.schema_version
            ),
        });
    }
    input.schema_version = SCHEMA_VERSION_V5;
    if input.reality_domains.is_empty() {
        input.reality_domains = default_seed_reality_domains();
    }
    Ok(input)
}

fn build_node_user_endpoint_memberships(
    state: &PersistedState,
) -> BTreeSet<NodeUserEndpointMembership> {
    normalize_node_user_endpoint_memberships(state, state.node_user_endpoint_memberships.clone())
}

fn normalize_node_user_endpoint_memberships(
    state: &PersistedState,
    memberships: BTreeSet<NodeUserEndpointMembership>,
) -> BTreeSet<NodeUserEndpointMembership> {
    let mut out = BTreeSet::new();
    for membership in memberships {
        if !state.users.contains_key(&membership.user_id) {
            continue;
        }
        let Some(endpoint) = state.endpoints.get(&membership.endpoint_id) else {
            continue;
        };
        // `node_id` is a redundant index; it must always follow the endpoint's node_id.
        out.insert(NodeUserEndpointMembership {
            user_id: membership.user_id,
            node_id: endpoint.node_id.clone(),
            endpoint_id: membership.endpoint_id,
        });
    }
    out
}

fn sync_node_user_endpoint_memberships(state: &mut PersistedState) {
    state.node_user_endpoint_memberships = build_node_user_endpoint_memberships(state);
}

fn build_node_user_endpoint_memberships_from_legacy_grants(
    state: &PersistedStateV9Compat,
) -> BTreeSet<NodeUserEndpointMembership> {
    let mut out = BTreeSet::new();
    for grant in state.grants.values() {
        let Some(endpoint) = state.endpoints.get(&grant.endpoint_id) else {
            continue;
        };
        if !state.users.contains_key(&grant.user_id) {
            continue;
        }
        out.insert(NodeUserEndpointMembership {
            user_id: grant.user_id.clone(),
            node_id: endpoint.node_id.clone(),
            endpoint_id: endpoint.endpoint_id.clone(),
        });
    }
    out
}
fn migrate_v5_to_v6(
    mut input: PersistedStateV9Compat,
) -> Result<PersistedStateV9Compat, StoreError> {
    if input.schema_version != SCHEMA_VERSION_V5 {
        return Err(StoreError::Migration {
            message: format!(
                "unexpected schema version for v5->v6 migration: {}",
                input.schema_version
            ),
        });
    }
    input.schema_version = SCHEMA_VERSION_V6;
    Ok(input)
}

fn migrate_v6_to_v7(
    mut input: PersistedStateV9Compat,
) -> Result<PersistedStateV9Compat, StoreError> {
    if input.schema_version != SCHEMA_VERSION_V6 {
        return Err(StoreError::Migration {
            message: format!(
                "unexpected schema version for v6->v7 migration: {}",
                input.schema_version
            ),
        });
    }
    input.schema_version = SCHEMA_VERSION_V7;
    input.node_user_endpoint_memberships =
        build_node_user_endpoint_memberships_from_legacy_grants(&input);
    Ok(input)
}

fn migrate_v7_to_v8(
    mut input: PersistedStateV9Compat,
) -> Result<PersistedStateV9Compat, StoreError> {
    if input.schema_version != SCHEMA_VERSION_V7 {
        return Err(StoreError::Migration {
            message: format!(
                "unexpected schema version for v7->v8 migration: {}",
                input.schema_version
            ),
        });
    }
    input.schema_version = SCHEMA_VERSION_V8;
    // Backward-compat: old schema only had node-scoped weights; treat nodes with any explicit
    // per-user node weight as "node override mode".
    for node_weights in input.user_node_weights.values() {
        for node_id in node_weights.keys() {
            input
                .node_weight_policies
                .entry(node_id.clone())
                .or_insert(NodeWeightPolicyConfig {
                    inherit_global: false,
                });
        }
    }
    Ok(input)
}

#[derive(Debug, Default, Clone)]
struct MigrateV9ToV10Stats {
    grants_total: usize,
    grants_orphan_dropped: usize,
    memberships_created: usize,
    memberships_deduped: usize,
    user_node_quotas_cleared: usize,
}

fn migrate_v9_compat_to_v10(
    input: PersistedStateV9Compat,
) -> Result<
    (
        PersistedState,
        BTreeMap<String, String>,
        MigrateV9ToV10Stats,
    ),
    StoreError,
> {
    let mut stats = MigrateV9ToV10Stats {
        grants_total: input.grants.len(),
        user_node_quotas_cleared: input.user_node_quotas.values().map(|m| m.len()).sum(),
        ..Default::default()
    };

    let mut out = PersistedState::empty();
    out.schema_version = SCHEMA_VERSION;
    out.nodes = input.nodes;
    out.endpoints = input.endpoints;
    out.endpoint_probe_history = input.endpoint_probe_history;
    out.users = input.users;
    out.reality_domains = input.reality_domains;
    // Hard cut: disable historical static overrides.
    out.user_node_quotas = BTreeMap::new();
    out.user_node_weights = input.user_node_weights;
    out.user_global_weights = input.user_global_weights;
    out.node_weight_policies = input.node_weight_policies;

    let mut grant_id_to_membership_key = BTreeMap::<String, String>::new();
    let mut membership_by_pair = BTreeMap::<(String, String), NodeUserEndpointMembership>::new();

    for (grant_id, grant) in input.grants {
        if !out.users.contains_key(&grant.user_id) {
            stats.grants_orphan_dropped += 1;
            continue;
        }
        let Some(endpoint) = out.endpoints.get(&grant.endpoint_id) else {
            stats.grants_orphan_dropped += 1;
            continue;
        };

        grant_id_to_membership_key
            .insert(grant_id, membership_key(&grant.user_id, &grant.endpoint_id));

        let pair_key = (grant.user_id.clone(), grant.endpoint_id.clone());
        let existed = membership_by_pair.contains_key(&pair_key);
        membership_by_pair
            .entry(pair_key)
            .or_insert_with(|| NodeUserEndpointMembership {
                user_id: grant.user_id.clone(),
                node_id: endpoint.node_id.clone(),
                endpoint_id: grant.endpoint_id.clone(),
            });
        if existed {
            stats.memberships_deduped += 1;
        }
    }

    out.node_user_endpoint_memberships = membership_by_pair.into_values().collect();
    stats.memberships_created = out.node_user_endpoint_memberships.len();

    // Normalize node_id indexes to match endpoints.
    sync_node_user_endpoint_memberships(&mut out);

    Ok((out, grant_id_to_membership_key, stats))
}

#[derive(Debug, Default, Clone)]
struct MigrateUsageV1ToV2Stats {
    grants_total: usize,
    grants_mapped: usize,
    grants_dropped_no_mapping: usize,
    memberships_created: usize,
    memberships_dropped_not_in_state: usize,
}

fn migrate_usage_v1_to_v2(
    input: PersistedUsageV1Compat,
    grant_id_to_membership_key: &BTreeMap<String, String>,
    allowed_membership_keys: &BTreeSet<String>,
) -> (PersistedUsage, MigrateUsageV1ToV2Stats) {
    let mut stats = MigrateUsageV1ToV2Stats {
        grants_total: input.grants.len(),
        ..Default::default()
    };

    let mut grouped = BTreeMap::<String, Vec<GrantUsageV1Compat>>::new();
    for (grant_id, usage) in input.grants {
        let Some(membership_key) = grant_id_to_membership_key.get(&grant_id) else {
            stats.grants_dropped_no_mapping += 1;
            continue;
        };
        stats.grants_mapped += 1;
        grouped
            .entry(membership_key.clone())
            .or_default()
            .push(usage);
    }

    let mut out = PersistedUsage {
        schema_version: USAGE_SCHEMA_VERSION,
        memberships: BTreeMap::new(),
        user_node_pacing: input.user_node_pacing,
        node_pacing: input.node_pacing,
        user_credential_epochs_applied: BTreeMap::new(),
        endpoint_users_applied: BTreeMap::new(),
    };

    for (membership_key, entries) in grouped {
        if !allowed_membership_keys.contains(&membership_key) {
            stats.memberships_dropped_not_in_state += 1;
            continue;
        }

        // Effective window is picked from the entry with max last_seen_at.
        let mut effective_start = String::new();
        let mut effective_end = String::new();
        let mut max_seen = None::<String>;
        for e in entries.iter() {
            if max_seen
                .as_deref()
                .is_none_or(|prev| e.last_seen_at.as_str() > prev)
            {
                max_seen = Some(e.last_seen_at.clone());
                effective_start = e.cycle_start_at.clone();
                effective_end = e.cycle_end_at.clone();
            }
        }

        let mut used_bytes = 0u64;
        let mut quota_banned = false;
        let mut quota_banned_at = None::<String>;
        let mut last_seen_at = String::new();

        for e in entries.iter() {
            if e.last_seen_at > last_seen_at {
                last_seen_at = e.last_seen_at.clone();
            }
            if e.cycle_start_at != effective_start || e.cycle_end_at != effective_end {
                continue;
            }

            used_bytes = used_bytes.saturating_add(e.used_bytes);
            quota_banned = quota_banned || e.quota_banned;
            if let Some(at) = &e.quota_banned_at
                && quota_banned_at
                    .as_deref()
                    .is_none_or(|prev| at.as_str() > prev)
            {
                quota_banned_at = Some(at.clone());
            }
        }

        out.memberships.insert(
            membership_key,
            MembershipUsage {
                cycle_start_at: effective_start,
                cycle_end_at: effective_end,
                used_bytes,
                // Force a baseline rebuild on the new email key to avoid negative deltas.
                last_uplink_total: 0,
                last_downlink_total: 0,
                last_seen_at,
                quota_banned,
                quota_banned_at,
            },
        );
    }

    stats.memberships_created = out.memberships.len();
    (out, stats)
}

fn normalize_user_global_weights(
    state: &PersistedState,
    global_weights: BTreeMap<String, UserGlobalWeightConfig>,
) -> BTreeMap<String, UserGlobalWeightConfig> {
    let mut out = BTreeMap::new();
    for (user_id, cfg) in global_weights {
        if state.users.contains_key(&user_id) {
            out.insert(user_id, cfg);
        }
    }
    out
}

fn normalize_node_weight_policies(
    state: &PersistedState,
    node_policies: BTreeMap<String, NodeWeightPolicyConfig>,
) -> BTreeMap<String, NodeWeightPolicyConfig> {
    let mut out = BTreeMap::new();
    for (node_id, cfg) in node_policies {
        if state.nodes.contains_key(&node_id) {
            out.insert(node_id, cfg);
        }
    }
    out
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
                quota_limit_bytes: 0,
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
                credentials: serde_json::json!({}),
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
        let mut v4 = PersistedStateV9Compat::empty_with_version(SCHEMA_VERSION_V4);
        v4.reality_domains = Vec::new();

        let v5 = migrate_v4_to_v5(v4).expect("migration should succeed");
        assert_eq!(v5.schema_version, SCHEMA_VERSION_V5);
        assert_eq!(v5.reality_domains, default_seed_reality_domains());
    }

    #[test]
    fn migrate_v4_to_v5_does_not_override_existing_reality_domains() {
        let mut v4 = PersistedStateV9Compat::empty_with_version(SCHEMA_VERSION_V4);
        v4.reality_domains = vec![RealityDomain {
            domain_id: "custom_1".to_string(),
            server_name: "example.com".to_string(),
            disabled_node_ids: BTreeSet::new(),
        }];

        let v5 = migrate_v4_to_v5(v4).expect("migration should succeed");
        assert_eq!(v5.schema_version, SCHEMA_VERSION_V5);
        assert_eq!(v5.reality_domains.len(), 1);
        assert_eq!(v5.reality_domains[0].domain_id, "custom_1");
        assert_eq!(v5.reality_domains[0].server_name, "example.com");
    }

    #[test]
    fn migrate_v6_to_v7_seeds_node_user_endpoint_memberships_from_grants() {
        let mut v6 = PersistedStateV9Compat::empty_with_version(SCHEMA_VERSION_V6);
        v6.users.insert(
            "user_1".to_string(),
            User {
                user_id: "user_1".to_string(),
                display_name: "alice".to_string(),
                subscription_token: "sub_1".to_string(),
                credential_epoch: 0,
                priority_tier: UserPriorityTier::P2,
                quota_reset: UserQuotaReset::default(),
            },
        );
        v6.nodes.insert(
            "node_1".to_string(),
            Node {
                node_id: "node_1".to_string(),
                node_name: "node-1".to_string(),
                access_host: "localhost".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            },
        );
        v6.endpoints.insert(
            "endpoint_1".to_string(),
            Endpoint {
                endpoint_id: "endpoint_1".to_string(),
                node_id: "node_1".to_string(),
                tag: "ep".to_string(),
                kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                port: 12345,
                meta: serde_json::json!({}),
            },
        );
        v6.grants.insert(
            "grant_1".to_string(),
            LegacyGrantCompat {
                grant_id: "grant_1".to_string(),
                user_id: "user_1".to_string(),
                endpoint_id: "endpoint_1".to_string(),
                enabled: true,
            },
        );

        let v7 = migrate_v6_to_v7(v6).expect("migration should succeed");
        assert_eq!(v7.schema_version, SCHEMA_VERSION_V7);
        assert!(
            v7.node_user_endpoint_memberships
                .contains(&NodeUserEndpointMembership {
                    user_id: "user_1".to_string(),
                    node_id: "node_1".to_string(),
                    endpoint_id: "endpoint_1".to_string(),
                })
        );
    }

    #[test]
    fn migrate_v7_to_v8_keeps_existing_weights_and_sets_latest_schema() {
        let mut v7 = PersistedStateV9Compat::empty_with_version(SCHEMA_VERSION_V7);
        v7.users.insert(
            "user_1".to_string(),
            User {
                user_id: "user_1".to_string(),
                display_name: "alice".to_string(),
                subscription_token: "sub_1".to_string(),
                credential_epoch: 0,
                priority_tier: UserPriorityTier::P2,
                quota_reset: UserQuotaReset::default(),
            },
        );
        v7.user_global_weights
            .insert("user_1".to_string(), UserGlobalWeightConfig { weight: 135 });

        let v8 = migrate_v7_to_v8(v7).expect("migration should succeed");
        assert_eq!(v8.schema_version, SCHEMA_VERSION_V8);
        assert_eq!(
            v8.user_global_weights.get("user_1"),
            Some(&UserGlobalWeightConfig { weight: 135 })
        );
    }

    #[test]
    fn migrate_v9_compat_to_v10_extracts_memberships_and_clears_user_node_quotas() {
        let mut v9 = PersistedStateV9Compat::empty_with_version(SCHEMA_VERSION_V9);

        v9.users.insert(
            "user_1".to_string(),
            User {
                user_id: "user_1".to_string(),
                display_name: "alice".to_string(),
                subscription_token: "sub_1".to_string(),
                credential_epoch: 0,
                priority_tier: UserPriorityTier::P2,
                quota_reset: UserQuotaReset::default(),
            },
        );
        v9.nodes.insert(
            "node_1".to_string(),
            Node {
                node_id: "node_1".to_string(),
                node_name: "node-1".to_string(),
                access_host: "localhost".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            },
        );
        v9.endpoints.insert(
            "endpoint_1".to_string(),
            Endpoint {
                endpoint_id: "endpoint_1".to_string(),
                node_id: "node_1".to_string(),
                tag: "ep".to_string(),
                kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                port: 12345,
                meta: serde_json::json!({}),
            },
        );

        v9.grants.insert(
            "grant_0".to_string(),
            LegacyGrantCompat {
                grant_id: "grant_0".to_string(),
                user_id: "user_1".to_string(),
                endpoint_id: "endpoint_1".to_string(),
                enabled: true,
            },
        );
        v9.grants.insert(
            "grant_1".to_string(),
            LegacyGrantCompat {
                grant_id: "grant_1".to_string(),
                user_id: "user_1".to_string(),
                endpoint_id: "endpoint_1".to_string(),
                enabled: false,
            },
        );
        v9.grants.insert(
            "grant_orphan_user".to_string(),
            LegacyGrantCompat {
                grant_id: "grant_orphan_user".to_string(),
                user_id: "user_missing".to_string(),
                endpoint_id: "endpoint_1".to_string(),
                enabled: true,
            },
        );
        v9.grants.insert(
            "grant_orphan_endpoint".to_string(),
            LegacyGrantCompat {
                grant_id: "grant_orphan_endpoint".to_string(),
                user_id: "user_1".to_string(),
                endpoint_id: "endpoint_missing".to_string(),
                enabled: true,
            },
        );

        v9.user_node_quotas.insert(
            "user_1".to_string(),
            BTreeMap::from([(
                "node_missing".to_string(),
                UserNodeQuotaConfig {
                    quota_limit_bytes: Some(123),
                    quota_reset_source: QuotaResetSource::User,
                },
            )]),
        );
        v9.user_node_quotas.insert(
            "user_missing".to_string(),
            BTreeMap::from([(
                "node_1".to_string(),
                UserNodeQuotaConfig {
                    quota_limit_bytes: Some(321),
                    quota_reset_source: QuotaResetSource::User,
                },
            )]),
        );

        let (v10, mapping, stats) = migrate_v9_compat_to_v10(v9).expect("migration should succeed");
        assert_eq!(v10.schema_version, SCHEMA_VERSION);
        assert!(v10.user_node_quotas.is_empty());
        assert_eq!(v10.node_user_endpoint_memberships.len(), 1);
        assert_eq!(
            mapping.get("grant_0"),
            Some(&"user_1::endpoint_1".to_string())
        );
        assert_eq!(
            mapping.get("grant_1"),
            Some(&"user_1::endpoint_1".to_string())
        );
        assert!(mapping.get("grant_orphan_user").is_none());
        assert!(mapping.get("grant_orphan_endpoint").is_none());
        assert_eq!(stats.grants_total, 4);
        assert_eq!(stats.grants_orphan_dropped, 2);
        assert_eq!(stats.memberships_created, 1);
        assert_eq!(stats.memberships_deduped, 1);
        assert_eq!(stats.user_node_quotas_cleared, 2);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
    SetUserNodeWeight {
        user_id: String,
        node_id: String,
        weight: u16,
    },
    SetUserGlobalWeight {
        user_id: String,
        weight: u16,
    },
    SetNodeWeightPolicy {
        node_id: String,
        inherit_global: bool,
    },
    SetUserMihomoProfile {
        user_id: String,
        profile: UserMihomoProfile,
    },
    /// Replace the user's access set (membership-only hard cut).
    ReplaceUserAccess {
        user_id: String,
        endpoint_ids: Vec<String>,
    },
    /// Ensure an access membership exists (idempotent; internal use).
    EnsureMembership {
        user_id: String,
        endpoint_id: String,
    },
    /// Bump `user.credential_epoch` to rotate derived credentials.
    BumpUserCredentialEpoch {
        user_id: String,
    },
    /// Legacy/WAL compatibility no-op.
    CompatNoop {
        note: String,
    },
    AppendEndpointProbeSamples {
        /// Hour bucket key like `2026-02-07T12:00:00Z`.
        hour: String,
        from_node_id: String,
        samples: Vec<EndpointProbeAppendSample>,
    },
}

// ---- WAL backward compatibility ----
//
// We keep legacy grants commands parseable so a node can upgrade and replay old WAL entries
// without requiring operators to boot an old binary for snapshot/purge.
//
// TODO(remove-compat): remove this shim after all clusters have upgraded to schema v10 and have
// completed at least one snapshot/purge cycle, then bump schema again (v11).

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum GrantEnabledSourceCompat {
    #[default]
    Manual,
    Quota,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct LegacyGrantCompat {
    #[serde(default)]
    grant_id: String,
    user_id: String,
    endpoint_id: String,
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct UserAccessItemCompat {
    endpoint_id: String,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DesiredStateCommandCompat {
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
    SetUserNodeWeight {
        user_id: String,
        node_id: String,
        weight: u16,
    },
    SetUserGlobalWeight {
        user_id: String,
        weight: u16,
    },
    SetNodeWeightPolicy {
        node_id: String,
        inherit_global: bool,
    },
    SetUserMihomoProfile {
        user_id: String,
        profile: UserMihomoProfile,
    },

    ReplaceUserAccess {
        user_id: String,
        // New shape (schema v10+): membership-only endpoint list.
        #[serde(default)]
        endpoint_ids: Vec<String>,
        // Legacy shape (schema v9): `items: [{ endpoint_id, note? }]`.
        #[serde(default)]
        items: Vec<UserAccessItemCompat>,
    },
    EnsureMembership {
        user_id: String,
        endpoint_id: String,
    },
    BumpUserCredentialEpoch {
        user_id: String,
    },
    CompatNoop {
        note: String,
    },

    AppendEndpointProbeSamples {
        hour: String,
        from_node_id: String,
        samples: Vec<EndpointProbeAppendSample>,
    },

    // Legacy grants commands (schema <= 9).
    ReplaceUserGrants {
        user_id: String,
        grants: Vec<LegacyGrantCompat>,
    },
    UpsertGrant {
        grant: LegacyGrantCompat,
    },
    DeleteGrant {
        grant_id: String,
    },
    SetGrantEnabled {
        grant_id: String,
        enabled: bool,
        #[serde(default)]
        source: GrantEnabledSourceCompat,
    },
}

impl<'de> Deserialize<'de> for DesiredStateCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let compat = DesiredStateCommandCompat::deserialize(deserializer)?;
        Ok(compat.into())
    }
}

impl From<DesiredStateCommandCompat> for DesiredStateCommand {
    fn from(value: DesiredStateCommandCompat) -> Self {
        match value {
            DesiredStateCommandCompat::UpsertNode { node } => Self::UpsertNode { node },
            DesiredStateCommandCompat::DeleteNode { node_id } => Self::DeleteNode { node_id },
            DesiredStateCommandCompat::UpsertEndpoint { endpoint } => {
                Self::UpsertEndpoint { endpoint }
            }
            DesiredStateCommandCompat::DeleteEndpoint { endpoint_id } => {
                Self::DeleteEndpoint { endpoint_id }
            }
            DesiredStateCommandCompat::CreateRealityDomain { domain } => {
                Self::CreateRealityDomain { domain }
            }
            DesiredStateCommandCompat::PatchRealityDomain {
                domain_id,
                server_name,
                disabled_node_ids,
            } => Self::PatchRealityDomain {
                domain_id,
                server_name,
                disabled_node_ids,
            },
            DesiredStateCommandCompat::DeleteRealityDomain { domain_id } => {
                Self::DeleteRealityDomain { domain_id }
            }
            DesiredStateCommandCompat::ReorderRealityDomains { domain_ids } => {
                Self::ReorderRealityDomains { domain_ids }
            }
            DesiredStateCommandCompat::UpsertUser { user } => Self::UpsertUser { user },
            DesiredStateCommandCompat::DeleteUser { user_id } => Self::DeleteUser { user_id },
            DesiredStateCommandCompat::ResetUserSubscriptionToken {
                user_id,
                subscription_token,
            } => Self::ResetUserSubscriptionToken {
                user_id,
                subscription_token,
            },
            DesiredStateCommandCompat::SetUserNodeQuota {
                user_id,
                node_id,
                quota_limit_bytes,
                quota_reset_source,
            } => Self::SetUserNodeQuota {
                user_id,
                node_id,
                quota_limit_bytes,
                quota_reset_source,
            },
            DesiredStateCommandCompat::SetUserNodeWeight {
                user_id,
                node_id,
                weight,
            } => Self::SetUserNodeWeight {
                user_id,
                node_id,
                weight,
            },
            DesiredStateCommandCompat::SetUserGlobalWeight { user_id, weight } => {
                Self::SetUserGlobalWeight { user_id, weight }
            }
            DesiredStateCommandCompat::SetNodeWeightPolicy {
                node_id,
                inherit_global,
            } => Self::SetNodeWeightPolicy {
                node_id,
                inherit_global,
            },
            DesiredStateCommandCompat::SetUserMihomoProfile { user_id, profile } => {
                Self::SetUserMihomoProfile { user_id, profile }
            }
            DesiredStateCommandCompat::ReplaceUserAccess {
                user_id,
                endpoint_ids,
                items,
            } => {
                // Support both v9 `items` and v10+ `endpoint_ids` WAL shapes.
                let mut merged: BTreeSet<String> = endpoint_ids.into_iter().collect();
                merged.extend(items.into_iter().map(|i| i.endpoint_id));
                Self::ReplaceUserAccess {
                    user_id,
                    endpoint_ids: merged.into_iter().collect(),
                }
            }
            DesiredStateCommandCompat::EnsureMembership {
                user_id,
                endpoint_id,
            } => Self::EnsureMembership {
                user_id,
                endpoint_id,
            },
            DesiredStateCommandCompat::BumpUserCredentialEpoch { user_id } => {
                Self::BumpUserCredentialEpoch { user_id }
            }
            DesiredStateCommandCompat::CompatNoop { note } => Self::CompatNoop { note },
            DesiredStateCommandCompat::AppendEndpointProbeSamples {
                hour,
                from_node_id,
                samples,
            } => Self::AppendEndpointProbeSamples {
                hour,
                from_node_id,
                samples,
            },

            DesiredStateCommandCompat::ReplaceUserGrants { user_id, grants } => {
                // Map legacy grants hard-cut to membership-only access list.
                let endpoint_ids = grants.into_iter().map(|g| g.endpoint_id).collect();
                Self::ReplaceUserAccess {
                    user_id,
                    endpoint_ids,
                }
            }
            DesiredStateCommandCompat::UpsertGrant { grant } => Self::EnsureMembership {
                user_id: grant.user_id,
                endpoint_id: grant.endpoint_id,
            },
            DesiredStateCommandCompat::DeleteGrant { grant_id } => Self::CompatNoop {
                note: format!("legacy delete_grant ignored: {grant_id}"),
            },
            DesiredStateCommandCompat::SetGrantEnabled {
                grant_id,
                enabled,
                source: _,
            } => Self::CompatNoop {
                note: format!(
                    "legacy set_grant_enabled ignored: grant_id={grant_id} enabled={enabled}"
                ),
            },
        }
    }
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
    UserAccessReplaced {
        created: usize,
        deleted: usize,
    },
    UserCredentialEpochBumped {
        user_id: String,
        credential_epoch: u32,
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

fn validate_node_quota_config(node: &Node) -> Result<(), DomainError> {
    // Shared node quota enforcement requires a finite cycle window.
    if node.quota_limit_bytes > 0 && matches!(node.quota_reset, NodeQuotaReset::Unlimited { .. }) {
        return Err(DomainError::InvalidNodeQuotaConfig {
            reason: "quota_limit_bytes > 0 requires quota_reset policy monthly".to_string(),
        });
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
                validate_node_quota_config(node)?;
                state.nodes.insert(node.node_id.clone(), node.clone());
                sync_node_user_endpoint_memberships(state);
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

                // A node-scoped weight config becomes meaningless once the node is removed.
                for (_user_id, nodes) in state.user_node_weights.iter_mut() {
                    nodes.remove(node_id);
                }
                state
                    .user_node_weights
                    .retain(|_user_id, nodes| !nodes.is_empty());
                state.node_weight_policies.remove(node_id);

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

                sync_node_user_endpoint_memberships(state);
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
                sync_node_user_endpoint_memberships(state);
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::DeleteEndpoint { endpoint_id } => {
                let deleted = state.endpoints.remove(endpoint_id).is_some();
                state.endpoint_probe_history.remove(endpoint_id);
                sync_node_user_endpoint_memberships(state);
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
                state.user_node_quotas.remove(user_id);
                state.user_node_weights.remove(user_id);
                state.user_global_weights.remove(user_id);
                state.user_mihomo_profiles.remove(user_id);
                sync_node_user_endpoint_memberships(state);
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

                Ok(DesiredStateApplyResult::UserNodeQuotaSet {
                    quota: UserNodeQuota {
                        user_id: user_id.clone(),
                        node_id: node_id.clone(),
                        quota_limit_bytes: *quota_limit_bytes,
                        quota_reset_source: quota_reset_source.clone(),
                    },
                })
            }
            Self::SetUserNodeWeight {
                user_id,
                node_id,
                weight,
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
                    .user_node_weights
                    .entry(user_id.clone())
                    .or_default()
                    .insert(node_id.clone(), UserNodeWeightConfig { weight: *weight });
                state.node_weight_policies.insert(
                    node_id.clone(),
                    NodeWeightPolicyConfig {
                        inherit_global: false,
                    },
                );

                Ok(DesiredStateApplyResult::Applied)
            }
            Self::SetUserGlobalWeight { user_id, weight } => {
                if !state.users.contains_key(user_id) {
                    return Err(DomainError::MissingUser {
                        user_id: user_id.clone(),
                    }
                    .into());
                }

                state
                    .user_global_weights
                    .insert(user_id.clone(), UserGlobalWeightConfig { weight: *weight });

                Ok(DesiredStateApplyResult::Applied)
            }
            Self::SetNodeWeightPolicy {
                node_id,
                inherit_global,
            } => {
                if !state.nodes.contains_key(node_id) {
                    return Err(DomainError::MissingNode {
                        node_id: node_id.clone(),
                    }
                    .into());
                }
                state.node_weight_policies.insert(
                    node_id.clone(),
                    NodeWeightPolicyConfig {
                        inherit_global: *inherit_global,
                    },
                );

                Ok(DesiredStateApplyResult::Applied)
            }
            Self::SetUserMihomoProfile { user_id, profile } => {
                if !state.users.contains_key(user_id) {
                    return Err(DomainError::MissingUser {
                        user_id: user_id.clone(),
                    }
                    .into());
                }
                state
                    .user_mihomo_profiles
                    .insert(user_id.clone(), profile.clone());
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::ReplaceUserAccess {
                user_id,
                endpoint_ids,
            } => {
                if !state.users.contains_key(user_id) {
                    return Err(DomainError::MissingUser {
                        user_id: user_id.clone(),
                    }
                    .into());
                }

                let desired_endpoint_ids: BTreeSet<String> = endpoint_ids.iter().cloned().collect();
                for endpoint_id in desired_endpoint_ids.iter() {
                    if !state.endpoints.contains_key(endpoint_id) {
                        return Err(DomainError::MissingEndpoint {
                            endpoint_id: endpoint_id.clone(),
                        }
                        .into());
                    }
                }

                let existing_endpoint_ids: BTreeSet<String> = state
                    .node_user_endpoint_memberships
                    .iter()
                    .filter(|m| m.user_id == *user_id)
                    .map(|m| m.endpoint_id.clone())
                    .collect();

                let created = desired_endpoint_ids
                    .difference(&existing_endpoint_ids)
                    .count();
                let deleted = existing_endpoint_ids
                    .difference(&desired_endpoint_ids)
                    .count();

                // Hard-cut semantics: the resulting memberships set for the user must be exactly
                // equal to the desired endpoint list.
                state
                    .node_user_endpoint_memberships
                    .retain(|m| m.user_id != *user_id);

                for endpoint_id in desired_endpoint_ids {
                    let endpoint = state
                        .endpoints
                        .get(&endpoint_id)
                        .expect("validated endpoint exists");
                    state
                        .node_user_endpoint_memberships
                        .insert(NodeUserEndpointMembership {
                            user_id: user_id.clone(),
                            node_id: endpoint.node_id.clone(),
                            endpoint_id: endpoint.endpoint_id.clone(),
                        });
                }

                sync_node_user_endpoint_memberships(state);
                Ok(DesiredStateApplyResult::UserAccessReplaced { created, deleted })
            }
            Self::EnsureMembership {
                user_id,
                endpoint_id,
            } => {
                if !state.users.contains_key(user_id) {
                    return Err(DomainError::MissingUser {
                        user_id: user_id.clone(),
                    }
                    .into());
                }
                let endpoint = state.endpoints.get(endpoint_id).ok_or_else(|| {
                    StoreError::Domain(DomainError::MissingEndpoint {
                        endpoint_id: endpoint_id.clone(),
                    })
                })?;
                state
                    .node_user_endpoint_memberships
                    .insert(NodeUserEndpointMembership {
                        user_id: user_id.clone(),
                        node_id: endpoint.node_id.clone(),
                        endpoint_id: endpoint.endpoint_id.clone(),
                    });
                sync_node_user_endpoint_memberships(state);
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::BumpUserCredentialEpoch { user_id } => {
                let user = state.users.get_mut(user_id).ok_or_else(|| {
                    StoreError::Domain(DomainError::MissingUser {
                        user_id: user_id.clone(),
                    })
                })?;
                user.credential_epoch = user.credential_epoch.saturating_add(1);
                Ok(DesiredStateApplyResult::UserCredentialEpochBumped {
                    user_id: user_id.clone(),
                    credential_epoch: user.credential_epoch,
                })
            }
            Self::CompatNoop { note: _ } => Ok(DesiredStateApplyResult::Applied),
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
                            skipped: sample.skipped,
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
    pub memberships: BTreeMap<String, MembershipUsage>,
    /// Local-only pacing state for shared node quota enforcement.
    ///
    /// Keyed by `(user_id, node_id)`.
    #[serde(default)]
    pub user_node_pacing: BTreeMap<String, BTreeMap<String, UserNodePacing>>,
    /// Local-only pacing state keyed by `node_id`.
    #[serde(default)]
    pub node_pacing: BTreeMap<String, NodePacing>,
    /// Local-only marker: last applied credential_epoch per user on this node.
    #[serde(default)]
    pub user_credential_epochs_applied: BTreeMap<String, u32>,
    /// Local-only cache: last applied desired users per endpoint on this node.
    ///
    /// Keyed by `endpoint_id`, values are `user_id` sets (excluding quota-banned memberships).
    #[serde(default)]
    pub endpoint_users_applied: BTreeMap<String, BTreeSet<String>>,
}

impl PersistedUsage {
    pub fn empty() -> Self {
        Self {
            schema_version: USAGE_SCHEMA_VERSION,
            memberships: BTreeMap::new(),
            user_node_pacing: BTreeMap::new(),
            node_pacing: BTreeMap::new(),
            user_credential_epochs_applied: BTreeMap::new(),
            endpoint_users_applied: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserNodePacing {
    pub bank_bytes: u64,
    #[serde(default)]
    pub last_total_used_bytes: u64,
    /// Last computed base quota for this `(user,node)` in bytes.
    ///
    /// This allows the quota engine to reconcile pacing immediately when policy inputs
    /// change mid-cycle (node quota, user weights, user tiers, etc.).
    #[serde(default)]
    pub last_base_quota_bytes: u64,
    /// Last seen user priority tier for this `(user,node)`.
    #[serde(default)]
    pub last_priority_tier: UserPriorityTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodePacing {
    pub cycle_start_at: String,
    pub cycle_end_at: String,
    pub last_day_index: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MembershipUsage {
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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct PersistedUsageV1Compat {
    pub schema_version: u32,
    #[serde(default)]
    pub grants: BTreeMap<String, GrantUsageV1Compat>,
    #[serde(default)]
    pub user_node_pacing: BTreeMap<String, BTreeMap<String, UserNodePacing>>,
    #[serde(default)]
    pub node_pacing: BTreeMap<String, NodePacing>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct GrantUsageV1Compat {
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
        let usage_path = init.data_dir.join("usage.json");
        let (mut state, grant_id_to_membership_key, is_new_state, mut migrated) =
            if state_path.exists() {
                let bytes = fs::read(&state_path)?;
                let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
                let schema_version = raw
                    .get("schema_version")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;

                match schema_version {
                    SCHEMA_VERSION => {
                        let v10: PersistedState = serde_json::from_value(raw)?;
                        (v10, None, false, false)
                    }
                    SCHEMA_VERSION_V9 | SCHEMA_VERSION_V8 | SCHEMA_VERSION_V7
                    | SCHEMA_VERSION_V6 | SCHEMA_VERSION_V5 | SCHEMA_VERSION_V4 | 3 => {
                        let mut legacy: PersistedStateV9Compat = serde_json::from_value(raw)?;
                        let mut migrated = false;

                        legacy = match schema_version {
                            3 => {
                                migrated = true;
                                migrate_v3_to_v4(legacy)?
                            }
                            _ => legacy,
                        };
                        legacy = if legacy.schema_version == SCHEMA_VERSION_V4 {
                            // v4 may have empty reality domains (seeded in v5).
                            migrated = true;
                            migrate_v4_to_v5(legacy)?
                        } else {
                            legacy
                        };
                        legacy = if legacy.schema_version == SCHEMA_VERSION_V5 {
                            migrated = true;
                            migrate_v5_to_v6(legacy)?
                        } else {
                            legacy
                        };
                        legacy = if legacy.schema_version == SCHEMA_VERSION_V6 {
                            migrated = true;
                            migrate_v6_to_v7(legacy)?
                        } else {
                            legacy
                        };
                        legacy = if legacy.schema_version == SCHEMA_VERSION_V7 {
                            migrated = true;
                            migrate_v7_to_v8(legacy)?
                        } else {
                            legacy
                        };

                        let (v10, mapping, stats) = migrate_v9_compat_to_v10(legacy)?;
                        // Best-effort migration stats (logs are useful in production upgrades).
                        tracing::info!(
                            grants_total = stats.grants_total,
                            grants_orphan_dropped = stats.grants_orphan_dropped,
                            memberships_created = stats.memberships_created,
                            memberships_deduped = stats.memberships_deduped,
                            user_node_quotas_cleared = stats.user_node_quotas_cleared,
                            "state migration: legacy->v10 (remove grants hard cut)"
                        );

                        let _ = migrated;
                        (v10, Some(mapping), false, true)
                    }
                    2 | 1 => {
                        let v2: PersistedStateV2Like = serde_json::from_value(raw)?;
                        let v4 = migrate_v2_like_to_v3(v2)?;
                        let v5 = migrate_v4_to_v5(v4)?;
                        let v6 = migrate_v5_to_v6(v5)?;
                        let v7 = migrate_v6_to_v7(v6)?;
                        let v8 = migrate_v7_to_v8(v7)?;

                        let (v10, mapping, stats) = migrate_v9_compat_to_v10(v8)?;
                        tracing::info!(
                            grants_total = stats.grants_total,
                            grants_orphan_dropped = stats.grants_orphan_dropped,
                            memberships_created = stats.memberships_created,
                            memberships_deduped = stats.memberships_deduped,
                            user_node_quotas_cleared = stats.user_node_quotas_cleared,
                            "state migration: v2like->v10 (remove grants hard cut)"
                        );
                        (v10, Some(mapping), false, true)
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
                    quota_limit_bytes: 0,
                    quota_reset: NodeQuotaReset::default(),
                };

                let mut state = PersistedState::empty();
                state.nodes.insert(node_id, node);
                state.reality_domains = default_seed_reality_domains();
                (state, None, true, false)
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

        let normalized_global_weights =
            normalize_user_global_weights(&state, state.user_global_weights.clone());
        if normalized_global_weights != state.user_global_weights {
            state.user_global_weights = normalized_global_weights;
            migrated = true;
        }

        let normalized_node_policies =
            normalize_node_weight_policies(&state, state.node_weight_policies.clone());
        if normalized_node_policies != state.node_weight_policies {
            state.node_weight_policies = normalized_node_policies;
            migrated = true;
        }

        let normalized_memberships = normalize_node_user_endpoint_memberships(
            &state,
            state.node_user_endpoint_memberships.clone(),
        );
        if normalized_memberships != state.node_user_endpoint_memberships {
            state.node_user_endpoint_memberships = normalized_memberships;
            migrated = true;
        }

        let expected_memberships = build_node_user_endpoint_memberships(&state);
        if expected_memberships != state.node_user_endpoint_memberships {
            state.node_user_endpoint_memberships = expected_memberships;
            migrated = true;
        }

        let allowed_membership_keys = state
            .node_user_endpoint_memberships
            .iter()
            .map(|m| membership_key(&m.user_id, &m.endpoint_id))
            .collect::<BTreeSet<_>>();

        let mut usage_migrated = false;
        let mut usage = if usage_path.exists() {
            let bytes = fs::read(&usage_path)?;
            let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
            let usage_schema_version = raw
                .get("schema_version")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            match usage_schema_version {
                USAGE_SCHEMA_VERSION => serde_json::from_value(raw)?,
                USAGE_SCHEMA_VERSION_V1 => {
                    match grant_id_to_membership_key.as_ref() {
                        Some(mapping) => {
                            let v1: PersistedUsageV1Compat = serde_json::from_value(raw)?;
                            let (v2, stats) =
                                migrate_usage_v1_to_v2(v1, mapping, &allowed_membership_keys);
                            tracing::info!(
                                grants_total = stats.grants_total,
                                grants_mapped = stats.grants_mapped,
                                grants_dropped_no_mapping = stats.grants_dropped_no_mapping,
                                memberships_created = stats.memberships_created,
                                memberships_dropped_not_in_state =
                                    stats.memberships_dropped_not_in_state,
                                "usage migration: v1(grants)->v2(memberships)"
                            );
                            migrated = true;
                            usage_migrated = true;
                            v2
                        }
                        None => {
                            // Recovery path: it's possible to end up with a v10 state and a v1
                            // usage file if the process crashes between saving them during an
                            // upgrade. In that case, the legacy grant mapping is no longer
                            // available, so we cannot safely migrate usage. Prefer booting with
                            // an empty v2 usage file over refusing to start.
                            tracing::warn!(
                                "usage migration: legacy grant mapping is missing; discarding v1 usage and resetting to v2 empty"
                            );
                            migrated = true;
                            usage_migrated = true;
                            PersistedUsage::empty()
                        }
                    }
                }
                got => {
                    return Err(StoreError::SchemaVersionMismatch {
                        expected: USAGE_SCHEMA_VERSION,
                        got,
                    });
                }
            }
        } else {
            PersistedUsage::empty()
        };

        // Always retain usage only for currently active memberships, even when usage is already v2.
        let before_usage = usage.memberships.len();
        usage
            .memberships
            .retain(|membership_key, _| allowed_membership_keys.contains(membership_key));
        if usage.memberships.len() != before_usage {
            migrated = true;
            usage_migrated = true;
        }

        let store = Self {
            state_path,
            state,
            usage_path,
            usage,
        };

        if is_new_state || migrated {
            store.save()?;
        }
        if usage_migrated {
            store.save_usage()?;
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

    pub fn update_usage<R>(
        &mut self,
        f: impl FnOnce(&mut PersistedUsage) -> R,
    ) -> Result<R, StoreError> {
        let out = f(&mut self.usage);
        self.save_usage()?;
        Ok(out)
    }

    pub fn get_membership_usage(&self, membership_key: &str) -> Option<MembershipUsage> {
        self.usage.memberships.get(membership_key).cloned()
    }

    pub fn clear_membership_usage(&mut self, membership_key: &str) -> Result<(), StoreError> {
        if self.usage.memberships.remove(membership_key).is_some() {
            self.save_usage()?;
        }
        Ok(())
    }

    pub fn set_quota_banned(
        &mut self,
        membership_key: &str,
        banned_at: String,
    ) -> Result<(), StoreError> {
        let entry = self
            .usage
            .memberships
            .entry(membership_key.to_string())
            .or_insert_with(|| MembershipUsage {
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

    pub fn clear_quota_banned(&mut self, membership_key: &str) -> Result<(), StoreError> {
        if let Some(entry) = self.usage.memberships.get_mut(membership_key) {
            entry.quota_banned = false;
            entry.quota_banned_at = None;
            self.save_usage()?;
        }
        Ok(())
    }

    pub fn get_user_credential_epoch_applied(&self, user_id: &str) -> u32 {
        self.usage
            .user_credential_epochs_applied
            .get(user_id)
            .copied()
            .unwrap_or(0)
    }

    pub fn set_user_credential_epoch_applied(
        &mut self,
        user_id: &str,
        credential_epoch: u32,
    ) -> Result<(), StoreError> {
        self.usage
            .user_credential_epochs_applied
            .insert(user_id.to_string(), credential_epoch);
        self.save_usage()?;
        Ok(())
    }

    pub fn get_endpoint_users_applied(&self, endpoint_id: &str) -> BTreeSet<String> {
        self.usage
            .endpoint_users_applied
            .get(endpoint_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_node_pacing(&self, node_id: &str) -> Option<NodePacing> {
        self.usage.node_pacing.get(node_id).cloned()
    }

    pub fn set_node_pacing(
        &mut self,
        node_id: String,
        pacing: NodePacing,
    ) -> Result<(), StoreError> {
        self.usage.node_pacing.insert(node_id, pacing);
        self.save_usage()?;
        Ok(())
    }

    pub fn clear_node_pacing(&mut self, node_id: &str) -> Result<(), StoreError> {
        if self.usage.node_pacing.remove(node_id).is_some() {
            self.save_usage()?;
        }
        Ok(())
    }

    pub fn get_user_node_pacing(&self, user_id: &str, node_id: &str) -> Option<UserNodePacing> {
        self.usage
            .user_node_pacing
            .get(user_id)
            .and_then(|m| m.get(node_id))
            .cloned()
    }

    pub fn set_user_node_pacing(
        &mut self,
        user_id: String,
        node_id: String,
        pacing: UserNodePacing,
    ) -> Result<(), StoreError> {
        self.usage
            .user_node_pacing
            .entry(user_id)
            .or_default()
            .insert(node_id, pacing);
        self.save_usage()?;
        Ok(())
    }

    pub fn clear_user_node_pacing_for_node(&mut self, node_id: &str) -> Result<(), StoreError> {
        let mut changed = false;
        for (_user_id, nodes) in self.usage.user_node_pacing.iter_mut() {
            if nodes.remove(node_id).is_some() {
                changed = true;
            }
        }
        self.usage
            .user_node_pacing
            .retain(|_user_id, nodes| !nodes.is_empty());
        if changed {
            self.save_usage()?;
        }
        Ok(())
    }

    pub fn apply_membership_usage_sample(
        &mut self,
        membership_key: &str,
        cycle_start_at: String,
        cycle_end_at: String,
        uplink_total: u64,
        downlink_total: u64,
        seen_at: String,
    ) -> Result<UsageSnapshot, StoreError> {
        let used_bytes = {
            let entry = self
                .usage
                .memberships
                .entry(membership_key.to_string())
                .or_insert_with(|| MembershipUsage {
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
            credential_epoch: 0,
            priority_tier: Default::default(),
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

    pub fn get_user_node_weight(&self, user_id: &str, node_id: &str) -> Option<u16> {
        self.state
            .user_node_weights
            .get(user_id)
            .and_then(|m| m.get(node_id).map(|cfg| cfg.weight))
    }

    pub fn get_user_global_weight(&self, user_id: &str) -> Option<u16> {
        self.state
            .user_global_weights
            .get(user_id)
            .map(|cfg| cfg.weight)
    }

    pub fn resolve_user_global_weight(&self, user_id: &str) -> u16 {
        self.get_user_global_weight(user_id)
            .unwrap_or(crate::quota_policy::DEFAULT_USER_NODE_WEIGHT)
    }

    pub fn is_node_weight_inherit_global(&self, node_id: &str) -> bool {
        self.state
            .node_weight_policies
            .get(node_id)
            .map(|cfg| cfg.inherit_global)
            .unwrap_or(true)
    }

    pub fn node_weight_policy_config(&self, node_id: &str) -> NodeWeightPolicyConfig {
        self.state
            .node_weight_policies
            .get(node_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn has_node_weight_policy(&self, node_id: &str) -> bool {
        self.state.node_weight_policies.contains_key(node_id)
    }

    pub fn resolve_user_node_weight(&self, user_id: &str, node_id: &str) -> u16 {
        let global_weight = self.resolve_user_global_weight(user_id);
        if self.is_node_weight_inherit_global(node_id) {
            return global_weight;
        }
        self.get_user_node_weight(user_id, node_id)
            .unwrap_or(global_weight)
    }

    pub fn list_user_node_weights(&self, user_id: &str) -> Result<Vec<(String, u16)>, StoreError> {
        if !self.state.users.contains_key(user_id) {
            return Err(DomainError::MissingUser {
                user_id: user_id.to_string(),
            }
            .into());
        }

        let mut out = Vec::new();
        if let Some(nodes) = self.state.user_node_weights.get(user_id) {
            for (node_id, cfg) in nodes {
                out.push((node_id.clone(), cfg.weight));
            }
        }
        Ok(out)
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

    pub fn get_user_mihomo_profile(&self, user_id: &str) -> Option<UserMihomoProfile> {
        self.state.user_mihomo_profiles.get(user_id).cloned()
    }

    pub fn get_user_by_subscription_token(&self, subscription_token: &str) -> Option<User> {
        self.state
            .users
            .values()
            .find(|u| u.subscription_token == subscription_token)
            .cloned()
    }

    pub fn list_node_users_with_endpoint_ids(&self, node_id: &str) -> Vec<(String, Vec<String>)> {
        let mut by_user = BTreeMap::<String, Vec<String>>::new();
        for membership in self.state.node_user_endpoint_memberships.iter() {
            if membership.node_id != node_id {
                continue;
            }
            by_user
                .entry(membership.user_id.clone())
                .or_default()
                .push(membership.endpoint_id.clone());
        }
        for endpoint_ids in by_user.values_mut() {
            endpoint_ids.sort();
            endpoint_ids.dedup();
        }
        by_user.into_iter().collect()
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

    pub fn list_user_access(
        &self,
        user_id: &str,
    ) -> Result<Vec<NodeUserEndpointMembership>, StoreError> {
        if !self.state.users.contains_key(user_id) {
            return Err(DomainError::MissingUser {
                user_id: user_id.to_string(),
            }
            .into());
        }
        let mut items: Vec<NodeUserEndpointMembership> = self
            .state
            .node_user_endpoint_memberships
            .iter()
            .filter(|m| m.user_id == user_id)
            .cloned()
            .collect();
        items.sort_by(|a, b| a.endpoint_id.cmp(&b.endpoint_id));
        Ok(items)
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
            DomainError, Endpoint, EndpointKind, Node, NodeQuotaReset, User, UserPriorityTier,
            UserQuotaReset, validate_cycle_day_of_month, validate_port,
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
    fn user_mihomo_profile_deserializes_legacy_template_yaml_alias() {
        let profile: UserMihomoProfile = serde_json::from_value(json!({
            "template_yaml": "port: 0\nrules: []\n",
            "extra_proxies_yaml": "",
            "extra_proxy_providers_yaml": ""
        }))
        .unwrap();

        assert_eq!(profile.mixin_yaml, "port: 0\nrules: []\n");

        let serialized = serde_json::to_value(&profile).unwrap();
        assert_eq!(serialized["mixin_yaml"], "port: 0\nrules: []\n");
        assert!(serialized.get("template_yaml").is_none());
    }

    #[test]
    fn replace_user_access_reports_delta_counts_not_physical_rewrites() {
        let mut state = PersistedState::empty();
        state.users.insert(
            "user_1".to_string(),
            User {
                user_id: "user_1".to_string(),
                display_name: "alice".to_string(),
                subscription_token: "sub_1".to_string(),
                credential_epoch: 0,
                priority_tier: UserPriorityTier::P2,
                quota_reset: UserQuotaReset::default(),
            },
        );
        state.nodes.insert(
            "node_1".to_string(),
            Node {
                node_id: "node_1".to_string(),
                node_name: "node-1".to_string(),
                access_host: "localhost".to_string(),
                api_base_url: "https://127.0.0.1:62416".to_string(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            },
        );
        for endpoint_id in ["endpoint_1", "endpoint_2", "endpoint_3"] {
            state.endpoints.insert(
                endpoint_id.to_string(),
                Endpoint {
                    endpoint_id: endpoint_id.to_string(),
                    node_id: "node_1".to_string(),
                    tag: endpoint_id.to_string(),
                    kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    port: 10_000,
                    meta: json!({}),
                },
            );
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
}
