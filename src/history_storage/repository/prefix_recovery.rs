use super::*;

impl HistoryStorage {
    /// Recovery never truncates an inventory: oversized ranges fail before returning any rows.
    pub(crate) fn prefix_recovery_records(
        &self,
        source: &str,
        epoch: u64,
        stream: &str,
        first: u64,
        last: u64,
    ) -> Result<Vec<RepositoryHistoryRecordRow>> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Err(HistoryStorageError(
                "prefix recovery requires SQLite".into(),
            ));
        };
        let mut statement = connection
            .prepare(
                "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
                    observer_node_id, schema_id, schema_version, record_key, is_tombstone,
                    observed_start, observed_end, received_at, payload
             FROM repository_history_records
             WHERE source_node_id=?1 AND source_epoch=?2 AND stream=?3
               AND sequence BETWEEN ?4 AND ?5
             ORDER BY sequence, subject_node_id, observer_node_id, schema_id, record_key
             LIMIT 129",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                params![
                    source,
                    durable_i64(epoch, "prefix epoch")?,
                    stream,
                    durable_i64(first, "prefix first")?,
                    durable_i64(last, "prefix last")?,
                ],
                repository_history_record_row,
            )
            .map_err(sqlite_error)?;
        let mut records = Vec::new();
        let mut bytes = 0_usize;
        for row in rows {
            let row = row.map_err(sqlite_error)?;
            bytes = bytes.saturating_add(row.payload.len());
            if records.len() == 128 || bytes > 192 * 1024 {
                return Err(HistoryStorageError(
                    "prefix inventory exceeds page bound".into(),
                ));
            }
            records.push(row);
        }
        Ok(records)
    }

    pub(crate) fn prefix_recovery_head_is_first(
        &self,
        source: &str,
        epoch: u64,
        stream: &str,
        sequence: u64,
    ) -> Result<bool> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Err(HistoryStorageError(
                "prefix recovery requires SQLite".into(),
            ));
        };
        let first: Option<i64> = connection
            .query_row(
                "SELECT MIN(first_sequence) FROM source_delivery_journal
             WHERE source_node_id=?1 AND source_epoch=?2 AND stream=?3",
                params![source, durable_i64(epoch, "prefix epoch")?, stream],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        Ok(first == Some(durable_i64(sequence, "prefix head")?))
    }
}
