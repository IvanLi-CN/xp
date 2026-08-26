use serde::{Deserialize, Serialize};

use crate::state::history_repository::control::RepositoryCapacity;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SourceDeliveryStatus {
    pub(super) state: String,
    pub(super) pending_segments: usize,
    pub(super) pending_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) oldest_pending_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) oldest_pending_age_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) last_acknowledged_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) last_delivery_path: Option<String>,
}

impl Default for SourceDeliveryStatus {
    fn default() -> Self {
        Self {
            state: "idle".to_owned(),
            pending_segments: 0,
            pending_bytes: 0,
            oldest_pending_cursor: None,
            oldest_pending_age_seconds: None,
            last_acknowledged_at: None,
            last_delivery_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryRuntimeStatus {
    pub(super) storage_mode: String,
    pub(super) capacity: RepositoryCapacity,
    pub(super) record_count: usize,
    pub(super) segment_count: usize,
    pub(super) gap_count: usize,
    pub(super) history_truncated: bool,
    pub(super) last_verified_unix_seconds: Option<u64>,
    pub(super) last_anti_entropy_unix_seconds: Option<u64>,
    pub(super) last_deep_verification_unix_seconds: Option<u64>,
    pub(super) last_dynamic_relay_attempt_unix_seconds: Option<u64>,
    #[serde(default)]
    pub(super) source_delivery: SourceDeliveryStatus,
}

impl RepositoryRuntimeStatus {
    pub(crate) fn capacity(&self) -> &RepositoryCapacity {
        &self.capacity
    }
}
