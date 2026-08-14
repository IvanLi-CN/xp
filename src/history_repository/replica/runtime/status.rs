use serde::{Deserialize, Serialize};

use crate::state::history_repository::control::RepositoryCapacity;

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
}

impl RepositoryRuntimeStatus {
    pub(crate) fn capacity(&self) -> &RepositoryCapacity {
        &self.capacity
    }
}
