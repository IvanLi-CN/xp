use super::{Result, sqlite_error};

pub(super) fn reset_legacy_byte_counters(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction
        .execute_batch(
            "UPDATE repository_delivery_metrics
             SET generated_segments = 0,
                 generated_bytes = 0,
                 acknowledged_segments = 0,
                 acknowledged_bytes = 0,
                 applied_segments = 0,
                 applied_bytes = 0,
                 source_delivery_attempts = 0,
                 source_delivery_successes = 0,
                 source_transport_errors = 0,
                 source_response_errors = 0,
                 source_response_changed = 0,
                 source_explicit_unavailable = 0,
                 source_storage_errors = 0,
                 last_attempt_at = NULL,
                 last_progress_at = NULL,
                 counter_epoch = counter_epoch + 1,
                 window_started_at = NULL,
                 window_generated_segments = NULL,
                 window_generated_bytes = NULL,
                 window_acknowledged_segments = NULL,
                 window_acknowledged_bytes = NULL,
                 window_applied_segments = NULL,
                 window_applied_bytes = NULL,
                 completed_window_started_at = NULL,
                 completed_window_ended_at = NULL,
                 completed_generated_segments = NULL,
                 completed_generated_bytes = NULL,
                 completed_acknowledged_segments = NULL,
                 completed_acknowledged_bytes = NULL,
                 completed_applied_segments = NULL,
                 completed_applied_bytes = NULL,
                 byte_counter_unit = 'canonical-v1'
             WHERE singleton = 1;
             DELETE FROM repository_delivery_metric_scopes;
             DELETE FROM repository_delivery_metric_scope_windows;",
        )
        .map_err(sqlite_error)
}
