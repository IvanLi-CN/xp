use super::sync::MAX_REPAIR_SEGMENTS;
use super::{RepositoryReplicaRuntime, RepositoryReplicaSummary, RepositoryRuntimeError};
use std::collections::BTreeSet;

impl RepositoryReplicaRuntime {
    pub(crate) fn missing_segment_page(
        &self,
        remote: &RepositoryReplicaSummary,
    ) -> Result<(Vec<String>, bool), RepositoryRuntimeError> {
        let local_ids = self
            .stored_segment_ids_by_ids(&remote.segment_ids)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut missing = remote
            .segment_ids
            .iter()
            .filter(|id| !local_ids.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let truncated = missing.len() > MAX_REPAIR_SEGMENTS;
        missing.truncate(MAX_REPAIR_SEGMENTS);
        Ok((missing, truncated))
    }
}
