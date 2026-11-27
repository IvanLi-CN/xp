//! Secondary indexes over the retained repository record table.
//!
//! Kept separate from the queries that name them so the schema stays scannable while each
//! `INDEXED BY` site remains one edit away from the index it depends on.

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
               ON repository_history_records (is_tombstone, received_at);
             CREATE INDEX IF NOT EXISTS repository_history_records_subject_start
               ON repository_history_records (subject_node_id, is_tombstone, observed_start);
             CREATE INDEX IF NOT EXISTS repository_history_records_subject_end
               ON repository_history_records (subject_node_id, is_tombstone, observed_end);
             CREATE INDEX IF NOT EXISTS repository_history_records_subject_received
               ON repository_history_records (subject_node_id, is_tombstone, received_at);";
