use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::state::history_repository::control::RepositoryCapacity;
use crate::state::history_storage::RepositoryDeliveryMetrics;

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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SummaryRepairResult {
    #[default]
    Idle,
    InProgress,
    Progress,
    ExplicitUnavailable,
    ResponseChanged,
    NoProgress,
    TransportError,
    StorageError,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SummaryRepairDiagnostic {
    #[serde(default)]
    pub(crate) pending_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) summary_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pending_next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_attempt_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_progress_at: Option<u64>,
    #[serde(default)]
    pub(crate) last_result: SummaryRepairResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) retry_after: Option<u64>,
    #[serde(default)]
    pub(crate) attempt_count: u64,
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
    #[serde(default)]
    pub(super) delivery_metrics: RepositoryDeliveryMetrics,
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) summary_repairs: BTreeMap<String, SummaryRepairDiagnostic>,
}

impl RepositoryRuntimeStatus {
    pub(crate) fn capacity(&self) -> &RepositoryCapacity {
        &self.capacity
    }
}
