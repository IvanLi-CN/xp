use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{
        CyclePolicy, CyclePolicyDefault, DomainError, Endpoint, EndpointKind, Grant,
        GrantCredentials, Node, Ss2022Credentials, User, VlessCredentials,
        validate_cycle_day_of_month, validate_port,
    },
    id::new_ulid_string,
    protocol::{
        RealityKeys, RotateShortIdResult, SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
        Ss2022EndpointMeta, VlessRealityVisionTcpEndpointMeta, generate_reality_keypair,
        generate_short_id_16hex, generate_ss2022_psk_b64, rotate_short_ids_in_place,
        ss2022_password,
    },
};

pub const SCHEMA_VERSION: u32 = 1;
pub const USAGE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct StoreInit {
    pub data_dir: PathBuf,
    pub bootstrap_node_name: String,
    pub bootstrap_public_domain: String,
    pub bootstrap_api_base_url: String,
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    SerdeJson(serde_json::Error),
    Domain(DomainError),
    SchemaVersionMismatch { expected: u32, got: u32 },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::SerdeJson(e) => write!(f, "json error: {e}"),
            Self::Domain(e) => write!(f, "{e}"),
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
    #[serde(default)]
    pub users: BTreeMap<String, User>,
    #[serde(default)]
    pub grants: BTreeMap<String, Grant>,
}

impl PersistedState {
    pub fn empty() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            nodes: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            users: BTreeMap::new(),
            grants: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesiredStateCommand {
    UpsertNode { node: Node },
    UpsertEndpoint { endpoint: Endpoint },
    DeleteEndpoint { endpoint_id: String },
    UpsertUser { user: User },
    DeleteUser { user_id: String },
    ResetUserSubscriptionToken {
        user_id: String,
        subscription_token: String,
    },
    UpsertGrant { grant: Grant },
    DeleteGrant { grant_id: String },
    UpdateGrantFields {
        grant_id: String,
        enabled: bool,
        quota_limit_bytes: u64,
        cycle_policy: CyclePolicy,
        cycle_day_of_month: Option<u8>,
    },
    SetGrantEnabled { grant_id: String, enabled: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesiredStateApplyResult {
    Applied,
    EndpointDeleted { deleted: bool },
    UserDeleted { deleted: bool },
    UserTokenReset { applied: bool },
    GrantDeleted { deleted: bool },
    GrantUpdated { grant: Option<Grant> },
    GrantEnabledSet { grant: Option<Grant>, changed: bool },
}

impl DesiredStateCommand {
    pub fn apply(
        &self,
        state: &mut PersistedState,
    ) -> Result<DesiredStateApplyResult, StoreError> {
        match self {
            Self::UpsertNode { node } => {
                state.nodes.insert(node.node_id.clone(), node.clone());
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::UpsertEndpoint { endpoint } => {
                validate_port(endpoint.port)?;
                state
                    .endpoints
                    .insert(endpoint.endpoint_id.clone(), endpoint.clone());
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::DeleteEndpoint { endpoint_id } => {
                let deleted = state.endpoints.remove(endpoint_id).is_some();
                Ok(DesiredStateApplyResult::EndpointDeleted { deleted })
            }
            Self::UpsertUser { user } => {
                validate_cycle_day_of_month(user.cycle_day_of_month_default)?;
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

                if grant.cycle_policy != CyclePolicy::InheritUser
                    && grant.cycle_day_of_month.is_none()
                {
                    return Err(DomainError::MissingCycleDayOfMonth {
                        cycle_policy: grant.cycle_policy.clone(),
                    }
                    .into());
                }
                if let Some(day) = grant.cycle_day_of_month {
                    validate_cycle_day_of_month(day)?;
                }

                state.grants.insert(grant.grant_id.clone(), grant.clone());
                Ok(DesiredStateApplyResult::Applied)
            }
            Self::DeleteGrant { grant_id } => {
                let deleted = state.grants.remove(grant_id).is_some();
                Ok(DesiredStateApplyResult::GrantDeleted { deleted })
            }
            Self::UpdateGrantFields {
                grant_id,
                enabled,
                quota_limit_bytes,
                cycle_policy,
                cycle_day_of_month,
            } => {
                let grant = match state.grants.get_mut(grant_id) {
                    Some(grant) => grant,
                    None => return Ok(DesiredStateApplyResult::GrantUpdated { grant: None }),
                };

                if *cycle_policy != CyclePolicy::InheritUser && cycle_day_of_month.is_none() {
                    return Err(DomainError::MissingCycleDayOfMonth {
                        cycle_policy: cycle_policy.clone(),
                    }
                    .into());
                }
                if let Some(day) = cycle_day_of_month {
                    validate_cycle_day_of_month(*day)?;
                }

                grant.enabled = *enabled;
                grant.quota_limit_bytes = *quota_limit_bytes;
                grant.cycle_policy = cycle_policy.clone();
                grant.cycle_day_of_month = *cycle_day_of_month;

                Ok(DesiredStateApplyResult::GrantUpdated {
                    grant: Some(grant.clone()),
                })
            }
            Self::SetGrantEnabled { grant_id, enabled } => {
                let grant = match state.grants.get_mut(grant_id) {
                    Some(grant) => grant,
                    None => return Ok(DesiredStateApplyResult::GrantEnabledSet {
                        grant: None,
                        changed: false,
                    }),
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
        let (state, is_new_state) = if state_path.exists() {
            let bytes = fs::read(&state_path)?;
            let state: PersistedState = serde_json::from_slice(&bytes)?;
            if state.schema_version != SCHEMA_VERSION {
                return Err(StoreError::SchemaVersionMismatch {
                    expected: SCHEMA_VERSION,
                    got: state.schema_version,
                });
            }
            (state, false)
        } else {
            let node_id = new_ulid_string();
            let node = Node {
                node_id: node_id.clone(),
                node_name: init.bootstrap_node_name,
                public_domain: init.bootstrap_public_domain,
                api_base_url: init.bootstrap_api_base_url,
            };

            let mut state = PersistedState::empty();
            state.nodes.insert(node_id, node);
            (state, true)
        };

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

        if is_new_state {
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

    pub fn create_endpoint(
        &mut self,
        node_id: String,
        kind: EndpointKind,
        port: u16,
        meta: serde_json::Value,
    ) -> Result<Endpoint, StoreError> {
        let endpoint_id = new_ulid_string();
        let tag = endpoint_tag(&kind, &endpoint_id);

        let meta = build_endpoint_meta(&kind, meta)?;
        let endpoint = Endpoint {
            endpoint_id: endpoint_id.clone(),
            node_id,
            tag,
            kind,
            port,
            meta,
        };
        DesiredStateCommand::UpsertEndpoint {
            endpoint: endpoint.clone(),
        }
        .apply(&mut self.state)?;
        self.save()?;
        Ok(endpoint)
    }

    pub fn create_user(
        &mut self,
        display_name: String,
        cycle_policy_default: CyclePolicyDefault,
        cycle_day_of_month_default: u8,
    ) -> Result<User, StoreError> {
        let user_id = new_ulid_string();
        let subscription_token = format!("sub_{}", new_ulid_string());

        let user = User {
            user_id: user_id.clone(),
            display_name,
            subscription_token,
            cycle_policy_default,
            cycle_day_of_month_default,
        };
        DesiredStateCommand::UpsertUser { user: user.clone() }.apply(&mut self.state)?;
        self.save()?;
        Ok(user)
    }

    pub fn create_grant(
        &mut self,
        user_id: String,
        endpoint_id: String,
        quota_limit_bytes: u64,
        cycle_policy: CyclePolicy,
        cycle_day_of_month: Option<u8>,
        note: Option<String>,
    ) -> Result<Grant, StoreError> {
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

        let grant_id = new_ulid_string();
        let credentials = credentials_for_endpoint(endpoint, &grant_id)?;

        let grant = Grant {
            grant_id: grant_id.clone(),
            user_id,
            endpoint_id,
            enabled: true,
            quota_limit_bytes,
            cycle_policy,
            cycle_day_of_month,
            note,
            credentials,
        };
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

    fn rotate_vless_reality_short_id_with_rng<R: rand::RngCore + rand::CryptoRng>(
        &mut self,
        endpoint_id: &str,
        rng: &mut R,
    ) -> Result<Option<RotateShortIdResult>, StoreError> {
        let mut endpoint = match self.state.endpoints.get(endpoint_id) {
            Some(endpoint) => endpoint.clone(),
            None => return Ok(None),
        };

        debug_assert_eq!(endpoint.kind, EndpointKind::VlessRealityVisionTcp);

        let mut meta: VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta.clone())?;

        let out = rotate_short_ids_in_place(&mut meta.short_ids, &mut meta.active_short_id, rng);

        endpoint.meta = serde_json::to_value(meta)?;
        DesiredStateCommand::UpsertEndpoint { endpoint }.apply(&mut self.state)?;
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

    pub fn update_grant(
        &mut self,
        grant_id: &str,
        enabled: bool,
        quota_limit_bytes: u64,
        cycle_policy: CyclePolicy,
        cycle_day_of_month: Option<u8>,
    ) -> Result<Option<Grant>, StoreError> {
        let out = DesiredStateCommand::UpdateGrantFields {
            grant_id: grant_id.to_string(),
            enabled,
            quota_limit_bytes,
            cycle_policy,
            cycle_day_of_month,
        }
        .apply(&mut self.state)?;
        let DesiredStateApplyResult::GrantUpdated { grant } = out else {
            unreachable!("update grant must return GrantUpdated");
        };
        if grant.is_some() {
            self.save()?;
        }
        Ok(grant)
    }

    pub fn set_grant_enabled(
        &mut self,
        grant_id: &str,
        enabled: bool,
    ) -> Result<Option<Grant>, StoreError> {
        let out = DesiredStateCommand::SetGrantEnabled {
            grant_id: grant_id.to_string(),
            enabled,
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
    public_domain: String,
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
                public_domain: input.public_domain,
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
            CyclePolicy, CyclePolicyDefault, DomainError, EndpointKind, Grant, GrantCredentials,
            validate_cycle_day_of_month, validate_port,
        },
        id::is_ulid_string,
        protocol::{RealityConfig, RealityKeys, VlessRealityVisionTcpEndpointMeta},
    };

    fn test_init(tmp_dir: &Path) -> StoreInit {
        StoreInit {
            data_dir: tmp_dir.to_path_buf(),
            bootstrap_node_name: "node-1".to_string(),
            bootstrap_public_domain: "".to_string(),
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
        assert_eq!(node.public_domain, "");
        assert_eq!(node.api_base_url, "https://127.0.0.1:62416");
        assert!(is_ulid_string(&node.node_id));
    }

    #[test]
    fn rotate_vless_reality_short_id_updates_meta_and_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();

        let node_id = store.list_nodes()[0].node_id.clone();
        let endpoint_id = "endpoint_1".to_string();
        let kind = EndpointKind::VlessRealityVisionTcp;

        let meta = VlessRealityVisionTcpEndpointMeta {
            public_domain: "example.com".to_string(),
            reality: RealityConfig {
                dest: "example.com:443".to_string(),
                server_names: vec!["example.com".to_string()],
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
        let meta: VlessRealityVisionTcpEndpointMeta = serde_json::from_value(endpoint.meta).unwrap();

        assert_eq!(out.active_short_id, meta.active_short_id);
        assert_eq!(out.short_ids, meta.short_ids);
    }

    #[test]
    fn save_load_roundtrip_persists_entities() {
        let tmp = tempfile::tempdir().unwrap();

        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();
        let user = store
            .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
            .unwrap();

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
                user_id: "user_1".to_string(),
                endpoint_id: "endpoint_1".to_string(),
                enabled: true,
                quota_limit_bytes: 0,
                cycle_policy: CyclePolicy::InheritUser,
                cycle_day_of_month: None,
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
            public_domain: "example.com".to_string(),
            api_base_url: "https://127.0.0.1:62416".to_string(),
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
            cycle_policy_default: CyclePolicyDefault::ByUser,
            cycle_day_of_month_default: 1,
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
        assert_eq!(out, DesiredStateApplyResult::UserTokenReset { applied: true });
        assert_eq!(
            state.users
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
                cycle_policy_default: CyclePolicyDefault::ByUser,
                cycle_day_of_month_default: 1,
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
            user_id: "user_1".to_string(),
            endpoint_id: "endpoint_1".to_string(),
            enabled: true,
            quota_limit_bytes: 10,
            cycle_policy: CyclePolicy::InheritUser,
            cycle_day_of_month: None,
            note: None,
            credentials: GrantCredentials {
                vless: None,
                ss2022: None,
            },
        };

        DesiredStateCommand::UpsertGrant { grant: grant.clone() }
            .apply(&mut state)
            .unwrap();
        assert_eq!(state.grants.get(&grant.grant_id), Some(&grant));

        let out = DesiredStateCommand::UpdateGrantFields {
            grant_id: grant.grant_id.clone(),
            enabled: false,
            quota_limit_bytes: 123,
            cycle_policy: CyclePolicy::InheritUser,
            cycle_day_of_month: None,
        }
        .apply(&mut state)
        .unwrap();

        let DesiredStateApplyResult::GrantUpdated { grant: updated } = out else {
            panic!("expected GrantUpdated");
        };
        let updated = updated.unwrap();
        assert!(!updated.enabled);
        assert_eq!(updated.quota_limit_bytes, 123);

        let out = DesiredStateCommand::DeleteGrant {
            grant_id: grant.grant_id.clone(),
        }
        .apply(&mut state)
        .unwrap();
        assert_eq!(out, DesiredStateApplyResult::GrantDeleted { deleted: true });
        assert!(!state.grants.contains_key(&grant.grant_id));
    }

    #[test]
    fn json_snapshot_store_create_update_delete_grant_flow_is_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = JsonSnapshotStore::load_or_init(test_init(tmp.path())).unwrap();

        let user = store
            .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
            .unwrap();
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
                user.user_id.clone(),
                endpoint.endpoint_id.clone(),
                1024,
                CyclePolicy::InheritUser,
                None,
                None,
            )
            .unwrap();

        store
            .set_quota_banned(&grant.grant_id, "2025-12-18T00:00:00Z".to_string())
            .unwrap();
        assert!(store.get_grant_usage(&grant.grant_id).is_some());

        let updated = store
            .update_grant(
                &grant.grant_id,
                false,
                2048,
                CyclePolicy::InheritUser,
                None,
            )
            .unwrap()
            .unwrap();
        assert!(!updated.enabled);
        assert_eq!(updated.quota_limit_bytes, 2048);

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
