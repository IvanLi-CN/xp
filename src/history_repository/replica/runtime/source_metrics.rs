use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn record_local_source_delivery_attempt(
        &mut self,
        attempt: crate::state::history_storage::SourceDeliveryAttempt,
    ) -> Result<(), RepositoryRuntimeError> {
        let result = self
            .storage
            .record_source_delivery_attempt(attempt)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()));
        if result.is_ok() {
            self.mark_delivery_metrics_dirty();
        }
        result
    }
}
