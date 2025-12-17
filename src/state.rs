use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        CyclePolicy, CyclePolicyDefault, DomainError, Endpoint, EndpointKind, Grant,
        GrantCredentials, Node, Ss2022Credentials, User, VlessCredentials,
        validate_cycle_day_of_month, validate_port,
    },
    id::new_ulid_string,
};

pub const SCHEMA_VERSION: u32 = 1;

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

pub struct JsonSnapshotStore {
    data_dir: PathBuf,
    state_path: PathBuf,
    state: PersistedState,
}

impl JsonSnapshotStore {
    pub fn load_or_init(init: StoreInit) -> Result<Self, StoreError> {
        fs::create_dir_all(&init.data_dir)?;

        let state_path = init.data_dir.join("state.json");
        if state_path.exists() {
            let bytes = fs::read(&state_path)?;
            let state: PersistedState = serde_json::from_slice(&bytes)?;
            if state.schema_version != SCHEMA_VERSION {
                return Err(StoreError::SchemaVersionMismatch {
                    expected: SCHEMA_VERSION,
                    got: state.schema_version,
                });
            }
            return Ok(Self {
                data_dir: init.data_dir,
                state_path,
                state,
            });
        }

        let node_id = new_ulid_string();
        let node = Node {
            node_id: node_id.clone(),
            node_name: init.bootstrap_node_name,
            public_domain: init.bootstrap_public_domain,
            api_base_url: init.bootstrap_api_base_url,
        };

        let mut state = PersistedState::empty();
        state.nodes.insert(node_id, node);

        let store = Self {
            data_dir: init.data_dir,
            state_path,
            state,
        };
        store.save()?;
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
        write_atomic(&self.data_dir, &self.state_path, &bytes)?;
        Ok(())
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
        let credentials = credentials_for_endpoint_kind(endpoint.kind.clone(), &grant_id);

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
}

fn endpoint_tag(kind: &EndpointKind, endpoint_id: &str) -> String {
    let kind_short = match kind {
        EndpointKind::VlessRealityVisionTcp => "vless-vision",
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => "ss2022",
    };
    format!("{kind_short}-{endpoint_id}")
}

fn credentials_for_endpoint_kind(kind: EndpointKind, grant_id: &str) -> GrantCredentials {
    match kind {
        EndpointKind::VlessRealityVisionTcp => GrantCredentials {
            vless: Some(VlessCredentials {
                uuid: new_ulid_string(),
                email: format!("grant:{grant_id}"),
            }),
            ss2022: None,
        },
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => GrantCredentials {
            vless: None,
            ss2022: Some(Ss2022Credentials {
                method: "2022-blake3-aes-128-gcm".to_string(),
                password: new_ulid_string(),
            }),
        },
    }
}

fn write_atomic(dir: &Path, path: &Path, bytes: &[u8]) -> Result<(), io::Error> {
    let tmp_path = dir.join("state.json.tmp");
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
