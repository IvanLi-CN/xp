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
                entry.used_bytes =
                    entry.used_bytes.saturating_add(delta_up.saturating_add(delta_down));
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
        validate_port(port)?;

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
        self.state.endpoints.insert(endpoint_id, endpoint.clone());
        self.save()?;
        Ok(endpoint)
    }

    pub fn create_user(
        &mut self,
        display_name: String,
        cycle_policy_default: CyclePolicyDefault,
        cycle_day_of_month_default: u8,
    ) -> Result<User, StoreError> {
        validate_cycle_day_of_month(cycle_day_of_month_default)?;

        let user_id = new_ulid_string();
        let subscription_token = format!("sub_{}", new_ulid_string());

        let user = User {
            user_id: user_id.clone(),
            display_name,
            subscription_token,
            cycle_policy_default,
            cycle_day_of_month_default,
        };
        self.state.users.insert(user_id, user.clone());
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

        if cycle_policy != CyclePolicy::InheritUser && cycle_day_of_month.is_none() {
            return Err(DomainError::MissingCycleDayOfMonth { cycle_policy }.into());
        }
        if let Some(day) = cycle_day_of_month {
            validate_cycle_day_of_month(day)?;
        }

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
        self.state.grants.insert(grant_id, grant.clone());
        self.save()?;
        Ok(grant)
    }

    pub fn list_nodes(&self) -> Vec<Node> {
        self.state.nodes.values().cloned().collect()
    }

    pub fn get_node(&self, node_id: &str) -> Option<Node> {
        self.state.nodes.get(node_id).cloned()
    }

    pub fn list_endpoints(&self) -> Vec<Endpoint> {
        self.state.endpoints.values().cloned().collect()
    }

    pub fn get_endpoint(&self, endpoint_id: &str) -> Option<Endpoint> {
        self.state.endpoints.get(endpoint_id).cloned()
    }

    pub fn delete_endpoint(&mut self, endpoint_id: &str) -> Result<bool, StoreError> {
        let deleted = self.state.endpoints.remove(endpoint_id).is_some();
        if deleted {
            self.save()?;
        }
        Ok(deleted)
    }

    pub fn rotate_vless_reality_short_id(
        &mut self,
        endpoint_id: &str,
    ) -> Result<Option<RotateShortIdResult>, StoreError> {
        let endpoint = match self.state.endpoints.get_mut(endpoint_id) {
            Some(endpoint) => endpoint,
            None => return Ok(None),
        };

        debug_assert_eq!(endpoint.kind, EndpointKind::VlessRealityVisionTcp);

        let mut meta: VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta.clone())?;

        let mut rng = rand::rngs::OsRng;
        let out =
            rotate_short_ids_in_place(&mut meta.short_ids, &mut meta.active_short_id, &mut rng);

        endpoint.meta = serde_json::to_value(meta)?;
        self.save()?;
        Ok(Some(out))
    }

    pub fn list_users(&self) -> Vec<User> {
        self.state.users.values().cloned().collect()
    }

    pub fn get_user(&self, user_id: &str) -> Option<User> {
        self.state.users.get(user_id).cloned()
    }

    pub fn delete_user(&mut self, user_id: &str) -> Result<bool, StoreError> {
        let deleted = self.state.users.remove(user_id).is_some();
        if deleted {
            self.save()?;
        }
        Ok(deleted)
    }

    pub fn reset_user_token(&mut self, user_id: &str) -> Result<Option<String>, StoreError> {
        let user = match self.state.users.get_mut(user_id) {
            Some(user) => user,
            None => return Ok(None),
        };

        let subscription_token = format!("sub_{}", new_ulid_string());
        user.subscription_token = subscription_token.clone();
        self.save()?;
        Ok(Some(subscription_token))
    }

    pub fn list_grants(&self) -> Vec<Grant> {
        self.state.grants.values().cloned().collect()
    }

    pub fn get_grant(&self, grant_id: &str) -> Option<Grant> {
        self.state.grants.get(grant_id).cloned()
    }

    pub fn delete_grant(&mut self, grant_id: &str) -> Result<bool, StoreError> {
        let deleted = self.state.grants.remove(grant_id).is_some();
        if deleted {
            self.save()?;
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
        let grant = match self.state.grants.get_mut(grant_id) {
            Some(grant) => grant,
            None => return Ok(None),
        };

        if cycle_policy != CyclePolicy::InheritUser && cycle_day_of_month.is_none() {
            return Err(DomainError::MissingCycleDayOfMonth { cycle_policy }.into());
        }
        if let Some(day) = cycle_day_of_month {
            validate_cycle_day_of_month(day)?;
        }

        grant.enabled = enabled;
        grant.quota_limit_bytes = quota_limit_bytes;
        grant.cycle_policy = cycle_policy;
        grant.cycle_day_of_month = cycle_day_of_month;

        let grant = grant.clone();
        self.save()?;
        Ok(Some(grant))
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

    use super::*;
    use crate::{
        domain::{CyclePolicyDefault, validate_cycle_day_of_month, validate_port},
        id::is_ulid_string,
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
}
