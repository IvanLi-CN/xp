use super::*;

impl HistoryStorage {
    /// Return one bounded keyset page for the persisted partition-summary rebuild. This is a
    /// maintenance path: callers may decode the payloads outside the HTTP summary request.
    pub(crate) fn repository_history_records_for_partition_summary(
        &self,
        after: Option<&RepositoryHistoryCompactionCursor>,
        limit: usize,
    ) -> Result<Vec<RepositoryHistoryRecordRow>> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(Vec::new());
        };
        let after = after.cloned().unwrap_or(RepositoryHistoryCompactionCursor {
            observed_start_unix_seconds: 0,
            source_node_id: String::new(),
            source_epoch: 0,
            stream: String::new(),
            sequence: 0,
        });
        let after_source_epoch = durable_i64(after.source_epoch, "summary cursor source epoch")?;
        let after_sequence = durable_i64(after.sequence, "summary cursor sequence")?;
        let mut statement = connection
            .prepare(
                "
                SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
                       observer_node_id, schema_id, schema_version, record_key, is_tombstone,
                       observed_start, observed_end, received_at, payload
                FROM repository_history_records INDEXED BY repository_history_records_keyset
                WHERE is_tombstone = 0
                  AND (observed_start, source_node_id, source_epoch, stream, sequence)
                      > (?1, ?2, ?3, ?4, ?5)
                ORDER BY observed_start, source_node_id, source_epoch, stream, sequence
                LIMIT ?6
                ",
            )
            .map_err(sqlite_error)?;
        let mapped_rows = statement
            .query_map(
                params![
                    i64::try_from(after.observed_start_unix_seconds).unwrap_or(i64::MAX),
                    after.source_node_id,
                    after_source_epoch,
                    after.stream,
                    after_sequence,
                    i64::try_from(limit).unwrap_or(i64::MAX),
                ],
                repository_history_record_row,
            )
            .map_err(sqlite_error)?;
        let mut rows = Vec::new();
        let mut payload_bytes = 0usize;
        for row in mapped_rows {
            let row = row.map_err(sqlite_error)?;
            let next_payload_bytes = payload_bytes.saturating_add(row.payload.len());
            if !rows.is_empty() && next_payload_bytes > 1024 * 1024 {
                break;
            }
            payload_bytes = next_payload_bytes;
            rows.push(row);
            if rows.len() >= limit.min(32) {
                break;
            }
        }
        Ok(rows)
    }
}
