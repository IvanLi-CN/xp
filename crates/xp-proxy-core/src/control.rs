use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationState {
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealitySnapshot {
    pub server_names: Vec<String>,
    pub short_ids: Vec<String>,
    pub private_key: String,
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
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlResponse {
    pub protocol_version: u16,
    pub request_id: String,
    pub generation: u64,
    pub state: GenerationState,
    pub error: Option<String>,
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
}

#[derive(Debug, Default)]
struct GenerationInner {
    active: Option<ProxySnapshot>,
    prepared: Option<ProxySnapshot>,
    state: GenerationState,
}

impl Default for GenerationState {
    fn default() -> Self {
        Self::Idle
    }
}

impl GenerationManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(GenerationInner::default())),
        }
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
        inner.state = GenerationState::Preparing;
        inner.prepared = Some(snapshot);
        inner.state = GenerationState::Ready;
        Ok(())
    }

    pub fn activate(&self) -> Result<u64, String> {
        let mut inner = self.inner.write().expect("generation lock poisoned");
        let snapshot = inner
            .prepared
            .take()
            .ok_or_else(|| "no prepared generation".to_string())?;
        let generation = snapshot.generation;
        inner.active = Some(snapshot);
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
}

impl ProxySnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_version == 0 {
            return Err("protocol_version must be non-zero".into());
        }
        if self.generation == 0 {
            return Err("generation must be non-zero".into());
        }
        if self.listeners.is_empty() {
            return Err("at least one listener is required".into());
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
            if listener.reality.server_names.is_empty() || listener.reality.short_ids.is_empty() {
                return Err("REALITY server_names and short_ids must not be empty".into());
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
            protocol_version: 1,
            generation: 0,
            listeners: vec![],
            users: vec![],
        };
        assert!(snapshot.validate().is_err());
    }
}
