use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use crate::metrics::TrafficSnapshot;

pub const CONTROL_PROTOCOL_VERSION: u16 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationState {
    #[default]
    Idle,
    Preparing,
    Validating,
    Starting,
    Ready,
    Active,
    Draining,
    Stopped,
    Rejected,
    Failed,
    RollbackRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxySnapshot {
    pub protocol_version: u16,
    pub generation: u64,
    pub listeners: Vec<ListenerSnapshot>,
    pub users: Vec<UserSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerSnapshot {
    pub bind: String,
    pub path: String,
    pub reality: RealitySnapshot,
    #[serde(default)]
    pub xmux: XmuxSnapshot,
    #[serde(default)]
    pub fallbacks: Vec<FallbackSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmuxSnapshot {
    pub max_connections: u16,
    pub max_concurrency: u16,
    pub c_max_reuse_times: u32,
    pub h_max_request_times: u32,
    pub h_max_reusable_secs: u32,
    pub h_keep_alive_period: i32,
}

impl Default for XmuxSnapshot {
    fn default() -> Self {
        Self {
            max_connections: 1,
            max_concurrency: 0,
            c_max_reuse_times: 0,
            h_max_request_times: 0,
            h_max_reusable_secs: 0,
            h_keep_alive_period: -1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackSnapshot {
    pub target: String,
    #[serde(default)]
    pub authorities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealitySnapshot {
    pub dest: String,
    pub server_names: Vec<String>,
    pub short_ids: Vec<String>,
    pub private_key: String,
}

impl RealitySnapshot {
    pub fn private_key_bytes(&self) -> Result<[u8; 32], String> {
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.private_key)
            .map_err(|_| "REALITY private_key must be base64url without padding".to_string())?;
        bytes
            .try_into()
            .map_err(|_| "REALITY private_key must decode to 32 bytes".to_string())
    }

    pub fn short_id_bytes(&self) -> Result<Vec<[u8; 8]>, String> {
        self.short_ids
            .iter()
            .map(|short_id| {
                let bytes = hex::decode(short_id)
                    .map_err(|_| format!("invalid REALITY short_id: {short_id}"))?;
                bytes
                    .try_into()
                    .map_err(|_| format!("REALITY short_id must be 8 bytes: {short_id}"))
            })
            .collect()
    }

    pub fn validate_dest(&self) -> Result<(), String> {
        crate::transport::RealityConfig {
            server_names: self.server_names.clone(),
            short_ids: self.short_ids.clone(),
            private_key: self.private_key_bytes()?,
            dest: self.dest.clone(),
        }
        .validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSnapshot {
    pub uuid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRequest {
    pub protocol_version: u16,
    pub request_id: String,
    pub generation: u64,
    pub operation: ControlOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub enum ControlOperation {
    Prepare { snapshot: ProxySnapshot },
    Activate,
    Drain,
    Stop,
    Rollback { generation: u64 },
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlResponse {
    pub protocol_version: u16,
    pub request_id: String,
    pub generation: u64,
    pub state: GenerationState,
    pub error: Option<String>,
    #[serde(default)]
    pub metrics: Option<TrafficSnapshot>,
}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let payload = serde_json::to_vec(value)?;
    let length = u32::try_from(payload.len())
        .unwrap_or(u32::MAX)
        .to_be_bytes();
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame<T: for<'de> Deserialize<'de>>(frame: &[u8]) -> Result<T, String> {
    if frame.len() < 4 {
        return Err("control frame is truncated".into());
    }
    let length = u32::from_be_bytes(frame[..4].try_into().expect("length slice")) as usize;
    if length != frame.len() - 4 {
        return Err("control frame length mismatch".into());
    }
    serde_json::from_slice(&frame[4..]).map_err(|error| error.to_string())
}

#[derive(Debug, Clone)]
pub struct GenerationManager {
    inner: Arc<RwLock<GenerationInner>>,
    persistence_path: Option<Arc<PathBuf>>,
}

#[derive(Debug, Default)]
struct GenerationInner {
    active: Option<ProxySnapshot>,
    previous: Option<ProxySnapshot>,
    prepared: Option<ProxySnapshot>,
    last_successful: Option<ProxySnapshot>,
    state: GenerationState,
}

impl GenerationManager {
    pub fn new() -> Self {
        Self::with_persistence(None).expect("in-memory generation manager must initialize")
    }

    pub fn new_with_persistence(path: impl Into<PathBuf>) -> Result<Self, String> {
        Self::with_persistence(Some(path.into()))
    }

    fn with_persistence(path: Option<PathBuf>) -> Result<Self, String> {
        let last_successful = path
            .as_deref()
            .filter(|path| path.exists())
            .map(|path| -> Result<ProxySnapshot, String> {
                let snapshot: ProxySnapshot = serde_json::from_slice(
                    &fs::read(path).map_err(|error| format!("read persisted snapshot: {error}"))?,
                )
                .map_err(|error| format!("decode persisted snapshot: {error}"))?;
                snapshot.validate()?;
                Ok(snapshot)
            })
            .transpose()?;
        Self {
            inner: Arc::new(RwLock::new(GenerationInner::default())),
            persistence_path: path.map(Arc::new),
        }
        .with_last_successful(last_successful)
    }

    fn with_last_successful(self, snapshot: Option<ProxySnapshot>) -> Result<Self, String> {
        self.inner
            .write()
            .map_err(|_| "generation lock poisoned".to_string())?
            .last_successful = snapshot;
        Ok(self)
    }

    pub fn prepare(&self, snapshot: ProxySnapshot) -> Result<(), String> {
        snapshot.validate()?;
        let mut inner = self.inner.write().expect("generation lock poisoned");
        if inner
            .active
            .as_ref()
            .is_some_and(|active| snapshot.generation <= active.generation)
        {
            return Err("generation must be newer than active generation".into());
        }
        if let Some(prepared) = inner.prepared.as_ref() {
            if prepared.generation != snapshot.generation {
                return Err("another generation is already prepared".into());
            }
            if prepared != &snapshot {
                return Err("prepared generation payload mismatch".into());
            }
            return Ok(());
        }
        inner.state = GenerationState::Preparing;
        inner.prepared = Some(snapshot);
        inner.state = GenerationState::Ready;
        Ok(())
    }

    pub fn activate(&self) -> Result<u64, String> {
        let mut inner = self.inner.write().expect("generation lock poisoned");
        let snapshot = inner
            .prepared
            .as_ref()
            .cloned()
            .ok_or_else(|| "no prepared generation".to_string())?;
        if let Some(path) = self.persistence_path.as_deref() {
            persist_snapshot(path, &snapshot)?;
        }
        let generation = snapshot.generation;
        inner.prepared.take();
        inner.last_successful = Some(snapshot.clone());
        inner.previous = inner.active.replace(snapshot);
        inner.state = GenerationState::Active;
        Ok(generation)
    }

    pub fn state(&self) -> (GenerationState, Option<u64>) {
        let inner = self.inner.read().expect("generation lock poisoned");
        (
            inner.state,
            inner.active.as_ref().map(|snapshot| snapshot.generation),
        )
    }

    pub fn active_snapshot(&self) -> Option<ProxySnapshot> {
        self.inner
            .read()
            .expect("generation lock poisoned")
            .active
            .clone()
    }

    pub fn prepared_snapshot(&self) -> Option<ProxySnapshot> {
        self.inner
            .read()
            .expect("generation lock poisoned")
            .prepared
            .clone()
    }

    pub fn last_successful_snapshot(&self) -> Option<ProxySnapshot> {
        self.inner
            .read()
            .expect("generation lock poisoned")
            .last_successful
            .clone()
    }

    pub fn drain(&self) -> Option<u64> {
        let mut inner = self.inner.write().expect("generation lock poisoned");
        if inner.active.is_some() {
            inner.state = GenerationState::Draining;
        }
        inner.active.as_ref().map(|snapshot| snapshot.generation)
    }

    pub fn stop(&self) -> Option<u64> {
        let mut inner = self.inner.write().expect("generation lock poisoned");
        let generation = inner.active.as_ref().map(|snapshot| snapshot.generation);
        if generation.is_some() {
            inner.state = GenerationState::Stopped;
        }
        generation
    }

    pub fn rollback(&self, expected_generation: u64) -> Result<u64, String> {
        let mut inner = self.inner.write().expect("generation lock poisoned");
        let active = inner
            .active
            .as_ref()
            .ok_or_else(|| "no active generation to rollback".to_string())?;
        if active.generation != expected_generation {
            return Err(format!(
                "rollback generation mismatch: active={}, expected={expected_generation}",
                active.generation
            ));
        }
        let previous = inner
            .previous
            .take()
            .ok_or_else(|| "no previous successful generation".to_string())?;
        if let Some(path) = self.persistence_path.as_deref() {
            persist_snapshot(path, &previous)?;
        }
        let generation = previous.generation;
        inner.last_successful = Some(previous.clone());
        inner.active = Some(previous);
        inner.state = GenerationState::Active;
        Ok(generation)
    }
}

fn persist_snapshot(path: &Path, snapshot: &ProxySnapshot) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "persisted snapshot has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create snapshot directory: {error}"))?;
    let mut directory_permissions = fs::metadata(parent)
        .map_err(|error| format!("stat snapshot directory: {error}"))?
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        directory_permissions.set_mode(0o700);
    }
    fs::set_permissions(parent, directory_permissions)
        .map_err(|error| format!("protect snapshot directory: {error}"))?;
    let temporary = path.with_extension("tmp");
    fs::write(
        &temporary,
        serde_json::to_vec(snapshot)
            .map_err(|error| format!("encode persisted snapshot: {error}"))?,
    )
    .map_err(|error| format!("write persisted snapshot: {error}"))?;
    let mut permissions = fs::metadata(&temporary)
        .map_err(|error| format!("stat persisted snapshot: {error}"))?
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o600);
    }
    fs::set_permissions(&temporary, permissions)
        .map_err(|error| format!("protect persisted snapshot: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("commit persisted snapshot: {error}"))
}

impl Default for GenerationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxySnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_version != CONTROL_PROTOCOL_VERSION {
            return Err(format!(
                "unsupported snapshot protocol version: {}",
                self.protocol_version
            ));
        }
        if self.generation == 0 {
            return Err("generation must be non-zero".into());
        }
        for listener in &self.listeners {
            if listener.bind.parse::<std::net::SocketAddr>().is_err() {
                return Err(format!("invalid listener bind: {}", listener.bind));
            }
            if !listener.path.starts_with('/') {
                return Err(format!(
                    "listener path must start with '/': {}",
                    listener.path
                ));
            }
            if listener.xmux.h_keep_alive_period < -1 {
                return Err("XMUX h_keep_alive_period must be -1 or non-negative".into());
            }
            if listener.reality.server_names.is_empty() || listener.reality.short_ids.is_empty() {
                return Err("REALITY server_names and short_ids must not be empty".into());
            }
            if listener.reality.short_ids.iter().any(|short_id| {
                short_id.len() != 16 || !short_id.bytes().all(|byte| byte.is_ascii_hexdigit())
            }) {
                return Err("REALITY short_id must be 16 hexadecimal characters".into());
            }
            listener.reality.private_key_bytes()?;
            listener.reality.short_id_bytes()?;
            listener.reality.validate_dest()?;
            for fallback in &listener.fallbacks {
                let target = crate::fallback::FallbackTarget::parse(&fallback.target)?;
                let authorities = fallback
                    .authorities
                    .iter()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                if !target.is_allowed(&authorities) {
                    return Err(format!(
                        "fallback target authority is not allowlisted: {}",
                        target.authority
                    ));
                }
            }
        }
        for user in &self.users {
            uuid::Uuid::parse_str(&user.uuid)
                .map_err(|_| format!("invalid VLESS UUID: {}", user.uuid))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_validation_rejects_invalid_generation() {
        let snapshot = ProxySnapshot {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            generation: 0,
            listeners: vec![],
            users: vec![],
        };
        assert!(snapshot.validate().is_err());
    }

    #[test]
    fn status_control_request_roundtrips_over_wire_format() {
        let request = ControlRequest {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            request_id: "status-test".into(),
            generation: 1,
            operation: ControlOperation::Status,
        };
        let decoded: ControlRequest = decode_frame(&encode_frame(&request).unwrap()).unwrap();
        assert_eq!(decoded, request);
    }

    fn snapshot(generation: u64) -> ProxySnapshot {
        ProxySnapshot {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            generation,
            listeners: vec![ListenerSnapshot {
                bind: "127.0.0.1:443".into(),
                path: "/xp-xhttp".into(),
                reality: RealitySnapshot {
                    dest: "127.0.0.1:443".into(),
                    server_names: vec!["example.test".into()],
                    short_ids: vec!["0123456789abcdef".into()],
                    private_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
                },
                xmux: XmuxSnapshot::default(),
                fallbacks: Vec::new(),
            }],
            users: vec![UserSnapshot {
                uuid: uuid::Uuid::nil().to_string(),
            }],
        }
    }

    #[test]
    fn failed_prepare_preserves_active_generation() {
        let manager = GenerationManager::new();
        manager.prepare(snapshot(1)).unwrap();
        manager.activate().unwrap();
        assert!(manager.prepare(snapshot(1)).is_err());
        assert_eq!(manager.state(), (GenerationState::Active, Some(1)));
    }

    #[test]
    fn rollback_restores_previous_generation_and_persists_it() {
        let directory = std::env::temp_dir().join(format!("xp-proxy-{}", uuid::Uuid::new_v4()));
        let path = directory.join("snapshot.json");
        let manager = GenerationManager::new_with_persistence(&path).unwrap();
        manager.prepare(snapshot(1)).unwrap();
        manager.activate().unwrap();
        manager.prepare(snapshot(2)).unwrap();
        manager.activate().unwrap();
        assert_eq!(manager.rollback(2).unwrap(), 1);
        assert_eq!(manager.state(), (GenerationState::Active, Some(1)));
        let restarted = GenerationManager::new_with_persistence(&path).unwrap();
        assert_eq!(restarted.last_successful_snapshot().unwrap().generation, 1);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn persistence_restores_diagnostics_without_reactivating_snapshot() {
        let directory = std::env::temp_dir().join(format!("xp-proxy-{}", uuid::Uuid::new_v4()));
        let path = directory.join("last-successful-snapshot.json");
        let manager = GenerationManager::new_with_persistence(&path).unwrap();
        manager.prepare(snapshot(7)).unwrap();
        manager.activate().unwrap();
        assert_eq!(manager.state(), (GenerationState::Active, Some(7)));

        let restarted = GenerationManager::new_with_persistence(&path).unwrap();
        assert_eq!(restarted.state(), (GenerationState::Idle, None));
        assert_eq!(restarted.last_successful_snapshot().unwrap().generation, 7);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn snapshot_validation_rejects_malformed_reality_key() {
        let mut invalid = snapshot(1);
        invalid.listeners[0].reality.private_key = "key".into();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn reality_snapshot_decodes_runtime_key_material() {
        let snapshot = snapshot(1);
        assert_eq!(
            snapshot.listeners[0].reality.private_key_bytes().unwrap(),
            [0; 32]
        );
        assert_eq!(
            snapshot.listeners[0].reality.short_id_bytes().unwrap(),
            vec![[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]]
        );
    }
}
