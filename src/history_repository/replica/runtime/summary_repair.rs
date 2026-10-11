use super::{RepositoryReplicaRuntime, RepositoryRuntimeError, SummaryRepairResult};

#[derive(Debug, Clone)]
pub(crate) struct SummaryRepairDiagnosticUpdate {
    pub(crate) now_unix_seconds: u64,
    pub(crate) result: SummaryRepairResult,
    pub(crate) error_code: Option<String>,
    pub(crate) retry_after: Option<u64>,
}

impl RepositoryReplicaRuntime {
    pub(crate) fn record_summary_repair_attempt(
        &mut self,
        peer_node_id: &str,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        let diagnostic = self
            .snapshot
            .summary_repair_diagnostics
            .entry(peer_node_id.to_owned())
            .or_default();
        diagnostic.last_attempt_at = Some(now_unix_seconds);
        diagnostic.last_result = SummaryRepairResult::InProgress;
        diagnostic.last_error_code = None;
        diagnostic.retry_after = None;
        diagnostic.attempt_count = diagnostic.attempt_count.saturating_add(1);
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn record_summary_repair_result(
        &mut self,
        peer_node_id: &str,
        now_unix_seconds: u64,
        result: SummaryRepairResult,
        error_code: Option<String>,
        retry_after: Option<u64>,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        self.record_summary_repair_result_in_memory(
            peer_node_id,
            now_unix_seconds,
            result,
            error_code,
            retry_after,
        );
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn record_summary_repair_result_in_memory(
        &mut self,
        peer_node_id: &str,
        now_unix_seconds: u64,
        result: SummaryRepairResult,
        error_code: Option<String>,
        retry_after: Option<u64>,
    ) {
        let diagnostic = self
            .snapshot
            .summary_repair_diagnostics
            .entry(peer_node_id.to_owned())
            .or_default();
        if matches!(
            result,
            SummaryRepairResult::Progress | SummaryRepairResult::ExplicitUnavailable
        ) {
            diagnostic.last_progress_at = Some(now_unix_seconds);
        }
        diagnostic.last_result = result;
        diagnostic.last_error_code = error_code;
        diagnostic.retry_after = retry_after;
    }

    pub(crate) fn persist_summary_repair_transition<T>(
        &mut self,
        peer_node_id: &str,
        update: SummaryRepairDiagnosticUpdate,
        transition: impl FnOnce(&mut Self) -> Result<T, RepositoryRuntimeError>,
    ) -> Result<T, RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        let value = match transition(self) {
            Ok(value) => value,
            Err(error) => {
                self.snapshot = previous_snapshot;
                return Err(error);
            }
        };
        self.record_summary_repair_result_in_memory(
            peer_node_id,
            update.now_unix_seconds,
            update.result,
            update.error_code,
            update.retry_after,
        );
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(value)
    }

    pub(super) fn sync_summary_repair_diagnostics(&mut self) {
        let checkpoints = self.snapshot.initial_peer_backfills.clone();
        for (peer_node_id, checkpoint) in checkpoints {
            let diagnostic = self
                .snapshot
                .summary_repair_diagnostics
                .entry(peer_node_id)
                .or_default();
            diagnostic.pending_count = checkpoint.summary_pending_segment_ids.len();
            diagnostic.summary_cursor = checkpoint.summary_cursor;
            diagnostic.pending_next_cursor = checkpoint.summary_pending_next_cursor;
        }
    }
}
