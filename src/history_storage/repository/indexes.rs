//! Secondary indexes over the retained repository record table.
//!
//! Kept separate from the queries that name them so the schema stays scannable while each
//! `INDEXED BY` site remains one edit away from the index it depends on.

use super::*;

/// Every secondary index over the retained record table.
///
/// Kept beside the queries that name them with `INDEXED BY`, so adding an index and the seek
/// that depends on it stay one edit. Each has a stated owner: the keyset/export/received triplet
/// serves whole-repository boundary seeks and paged reads, `coverage` serves subject completeness,
/// and the `subject_*` triplet gives each subject-scoped boundary its own index edge.
pub(crate) const REPOSITORY_HISTORY_RECORD_INDEXES: &str = "
CREATE INDEX IF NOT EXISTS repository_history_records_tombstone
              ON repository_history_records
                 (source_node_id, source_epoch, stream, subject_node_id, observer_node_id,
                  schema_id, schema_version, record_key, is_tombstone);
             CREATE INDEX IF NOT EXISTS repository_history_records_tombstone_target
               ON repository_history_records
                  (subject_node_id, observer_node_id, schema_id, schema_version,
                   is_tombstone, record_key);
             CREATE INDEX IF NOT EXISTS repository_history_records_aggregate_completeness
               ON repository_history_records
                  (subject_node_id, aggregate_complete, aggregate_start, aggregate_end);
             CREATE INDEX IF NOT EXISTS repository_history_records_keyset
               ON repository_history_records
                  (is_tombstone, observed_start, source_node_id, source_epoch, stream, sequence,
                   observed_end, received_at);
             CREATE INDEX IF NOT EXISTS repository_history_records_export_filter
               ON repository_history_records
                  (is_tombstone, observed_end, received_at, observed_start, source_node_id,
                   source_epoch, stream, sequence);
             CREATE INDEX IF NOT EXISTS repository_history_records_coverage
               ON repository_history_records
                  (subject_node_id, is_tombstone, observed_start, observed_end, received_at);
             CREATE INDEX IF NOT EXISTS repository_history_records_received
               ON repository_history_records (is_tombstone, received_at);";

/// Subject-scoped coverage indexes, built by a background worker rather than startup.
///
/// On a multi-gigabyte repository each build scans every retained row. Container readiness waits
/// only twenty seconds, so building these during preparation made `xp` miss its own health check
/// and restart in a loop until the image was rolled back. Coverage stays correct without them,
/// just slower, so the page-index worker owns them once the API is serving.
pub(crate) const REPOSITORY_HISTORY_SUBJECT_INDEXES: &str = "
             CREATE INDEX IF NOT EXISTS repository_history_records_subject_start
               ON repository_history_records (subject_node_id, is_tombstone, observed_start);
             CREATE INDEX IF NOT EXISTS repository_history_records_subject_end
               ON repository_history_records (subject_node_id, is_tombstone, observed_end);
             CREATE INDEX IF NOT EXISTS repository_history_records_subject_received
               ON repository_history_records (subject_node_id, is_tombstone, received_at);";

impl super::HistoryStorage {
    /// Build the subject coverage indexes, or report that they are already present.
    ///
    /// Returns `true` when this call created any of them. Startup never calls this: each index
    /// scans every retained row, and container readiness waits only twenty seconds, so a slow
    /// build would miss the health check and be rolled back before it committed.
    pub(crate) fn build_repository_subject_indexes(&self) -> Result<bool> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(false);
        };
        if query_sql::subject_indexes_present(connection)? {
            return Ok(false);
        }
        connection
            .execute_batch(indexes::REPOSITORY_HISTORY_SUBJECT_INDEXES)
            .map_err(sqlite_error)?;
        Ok(true)
    }
}
