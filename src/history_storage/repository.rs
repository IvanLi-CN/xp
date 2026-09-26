use serde::{Deserialize, Serialize};

use super::*;
mod diagnostics;
mod segments;
mod summary;
#[cfg(test)]
pub(crate) use segments::segment_phase_sql;
/// A repository-history row is deliberately stored outside the control snapshot.
/// The metadata columns keep retention and paged queries in SQLite rather than loading the
/// two-year repository window into the replica process.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RepositoryHistoryRecordRow {
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
    pub(crate) sequence: u64,
    pub(crate) subject_node_id: String,
    pub(crate) observer_node_id: String,
    pub(crate) schema_id: String,
    pub(crate) schema_version: u32,
    pub(crate) record_key: Vec<u8>,
    pub(crate) tombstone: bool,
    pub(crate) observed_start_unix_seconds: u64,
    pub(crate) observed_end_unix_seconds: u64,
    pub(crate) received_at_unix_seconds: u64,
    pub(crate) aggregate_complete: Option<bool>,
    pub(crate) aggregate_start_unix_seconds: Option<u64>,
    pub(crate) aggregate_end_unix_seconds: Option<u64>,
    pub(crate) payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryHistoryCompactionCursor {
    pub(crate) observed_start_unix_seconds: u64,
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
    pub(crate) sequence: u64,
}

pub(crate) type RepositoryHistoryExportWatermarks = (
    Option<RepositoryHistoryCompactionCursor>,
    Option<RepositoryHistoryCompactionCursor>,
    u64,
);

impl From<&RepositoryHistoryRecordRow> for RepositoryHistoryCompactionCursor {
    fn from(row: &RepositoryHistoryRecordRow) -> Self {
        Self {
            observed_start_unix_seconds: row.observed_start_unix_seconds,
            source_node_id: row.source_node_id.clone(),
            source_epoch: row.source_epoch,
            stream: row.stream.clone(),
            sequence: row.sequence,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryHistoryTombstone {
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
    pub(crate) subject_node_id: String,
    pub(crate) observer_node_id: String,
    pub(crate) schema_id: String,
    pub(crate) schema_version: u32,
    pub(crate) record_key: Vec<u8>,
    pub(crate) prefix: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryHistorySegmentRow {
    pub(crate) id: String,
    pub(crate) closed_at_unix_seconds: u64,
    pub(crate) contains_tombstone: bool,
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
    pub(crate) first_sequence: u64,
    pub(crate) payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryHistorySegmentMetadataRow {
    pub(crate) id: String,
    pub(crate) contains_tombstone: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RepositoryCommitOutcome {
    pub(crate) maintenance_degraded: bool,
}

/// All durable changes caused by accepting one repository segment. Keeping these changes with
/// the receiver checkpoint in one SQLite transaction prevents a failed checkpoint from leaving
/// accepted rows, a deleted payload, or an orphaned segment behind.
#[derive(Debug, Default)]
pub(crate) struct RepositoryReplicaMutation {
    pub(crate) records: Vec<RepositoryHistoryRecordRow>,
    pub(crate) tombstones: Vec<RepositoryHistoryTombstone>,
    pub(crate) segments: Vec<RepositoryHistorySegmentRow>,
}

fn sqlite_nullable_integer(value: rusqlite::types::ValueRef<'_>) -> Option<Option<i64>> {
    match value {
        rusqlite::types::ValueRef::Null => Some(None),
        rusqlite::types::ValueRef::Integer(value) => Some(Some(value)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RepositoryHistoryCoverage {
    pub(crate) observed_start_unix_seconds: u64,
    pub(crate) observed_end_unix_seconds: u64,
    pub(crate) received_start_unix_seconds: u64,
    pub(crate) received_end_unix_seconds: u64,
}

impl HistoryStorage {
    pub(crate) fn allocate_repository_source_epoch(
        &self,
        cluster_id: &str,
        node_id: &str,
        initial_epoch: u64,
    ) -> Result<u64> {
        let key = repository_source_epoch_meta_key(cluster_id, node_id);
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(initial_epoch.max(1));
        };
        let previous = read_meta_i64(connection, &key)?.and_then(|value| u64::try_from(value).ok());
        let epoch = match previous {
            Some(value) => value.checked_add(1).ok_or_else(|| {
                HistoryStorageError("repository source epoch exhausted".to_owned())
            })?,
            None => initial_epoch.max(1),
        };
        let stored_epoch = i64::try_from(epoch)
            .map_err(|_| HistoryStorageError("repository source epoch exhausted".to_owned()))?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        write_meta_i64(&transaction, &key, stored_epoch)?;
        transaction.commit().map_err(sqlite_error)?;
        Ok(epoch)
    }

    pub(crate) fn record_repository_source_epoch(
        &self,
        cluster_id: &str,
        node_id: &str,
        epoch: u64,
    ) -> Result<()> {
        let key = repository_source_epoch_meta_key(cluster_id, node_id);
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(());
        };
        let stored_epoch = i64::try_from(epoch)
            .map_err(|_| HistoryStorageError("repository source epoch exhausted".to_owned()))?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        write_meta_i64(&transaction, &key, stored_epoch)?;
        transaction.commit().map_err(sqlite_error)
    }

    pub(crate) fn repository_history_used_bytes_with_caller(
        &self,
        caller_class: &'static str,
    ) -> Result<u64> {
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::RepositoryHistoryUsedBytes,
            caller_class,
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish();
            return Ok(0);
        };
        let result = (|| {
            let page_count = connection
                .pragma_query_value(None, "page_count", |row| row.get::<_, i64>(0))
                .map_err(sqlite_error)?;
            let page_size = connection
                .pragma_query_value(None, "page_size", |row| row.get::<_, i64>(0))
                .map_err(sqlite_error)?;
            let database_bytes = u64::try_from(page_count)
                .unwrap_or(u64::MAX)
                .saturating_mul(u64::try_from(page_size).unwrap_or(u64::MAX));
            let wal_bytes = fs::metadata(self.data_dir.join(format!("{SQLITE_FILE}-wal")))
                .map(|metadata| metadata.len())
                .unwrap_or_default();
            Ok(database_bytes.saturating_add(wal_bytes))
        })();
        if result.is_ok() {
            diagnostic.finish();
        }
        result
    }

    #[allow(dead_code)]
    pub(crate) fn upsert_repository_history_records(
        &self,
        rows: &[RepositoryHistoryRecordRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Err(HistoryStorageError(
                "repository history row storage requires SQLite".to_owned(),
            ));
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        for row in rows {
            transaction
                .execute(
                    "
                    INSERT INTO repository_history_records (
                        source_node_id, source_epoch, stream, sequence, subject_node_id,
                        observer_node_id, schema_id, schema_version, record_key, is_tombstone,
                        observed_start, observed_end, received_at, aggregate_complete,
                        aggregate_start, aggregate_end, payload
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                        ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17
                    )
                    ON CONFLICT(source_node_id, source_epoch, stream, sequence) DO UPDATE SET
                        subject_node_id = excluded.subject_node_id,
                        observer_node_id = excluded.observer_node_id,
                        schema_id = excluded.schema_id,
                        schema_version = excluded.schema_version,
                        record_key = excluded.record_key,
                        is_tombstone = excluded.is_tombstone,
                        observed_start = excluded.observed_start,
                        observed_end = excluded.observed_end,
                        received_at = excluded.received_at,
                        aggregate_complete = excluded.aggregate_complete,
                        aggregate_start = excluded.aggregate_start,
                        aggregate_end = excluded.aggregate_end,
                        payload = excluded.payload
                    ",
                    params![
                        row.source_node_id,
                        durable_i64(row.source_epoch, "source epoch")?,
                        row.stream,
                        durable_i64(row.sequence, "sequence")?,
                        row.subject_node_id,
                        row.observer_node_id,
                        row.schema_id,
                        i64::from(row.schema_version),
                        row.record_key,
                        row.tombstone,
                        i64::try_from(row.observed_start_unix_seconds).unwrap_or(i64::MAX),
                        i64::try_from(row.observed_end_unix_seconds).unwrap_or(i64::MAX),
                        i64::try_from(row.received_at_unix_seconds).unwrap_or(i64::MAX),
                        row.aggregate_complete,
                        row.aggregate_start_unix_seconds
                            .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        row.aggregate_end_unix_seconds
                            .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        row.payload,
                    ],
                )
                .map_err(sqlite_error)?;
        }
        transaction.commit().map_err(sqlite_error)?;
        maintain_sqlite(connection)
    }

    pub(crate) fn commit_repository_replica_mutation(
        &self,
        mutation: RepositoryReplicaMutation,
        control_payload: &[u8],
    ) -> Result<RepositoryCommitOutcome> {
        #[cfg(test)]
        let fail_maintenance = self
            .fail_maintenance_after_commit
            .load(std::sync::atomic::Ordering::Relaxed);
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Err(HistoryStorageError(
                "repository replica mutation requires SQLite".to_owned(),
            ));
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        for tombstone in &mutation.tombstones {
            delete_repository_history_for_tombstone(&transaction, tombstone)?;
        }
        for row in &mutation.records {
            upsert_repository_history_record(&transaction, row)?;
        }
        for row in &mutation.segments {
            upsert_repository_history_segment(&transaction, row)?;
        }
        write_snapshot(&transaction, REPOSITORY_REPLICA_KEY, control_payload)?;
        transaction.commit().map_err(sqlite_error)?;
        #[cfg(test)]
        let maintenance_result = if fail_maintenance {
            Err(HistoryStorageError(
                "injected post-commit maintenance failure".to_owned(),
            ))
        } else {
            maintain_sqlite(connection)
        };
        #[cfg(not(test))]
        let maintenance_result = maintain_sqlite(connection);
        let maintenance_degraded = finish_post_commit_maintenance(maintenance_result);
        Ok(RepositoryCommitOutcome {
            maintenance_degraded,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn delete_repository_history_for_tombstone(
        &self,
        tombstone: &RepositoryHistoryTombstone,
    ) -> Result<usize> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(0);
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        let affected = delete_repository_history_for_tombstone(&transaction, tombstone)?;
        transaction.commit().map_err(sqlite_error)?;
        maintain_sqlite(connection)?;
        Ok(affected)
    }

    pub(crate) fn delete_repository_history_tombstone(
        &self,
        tombstone: &RepositoryHistoryTombstone,
    ) -> Result<()> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(());
        };
        connection
            .execute(
                "DELETE FROM repository_history_records
                 WHERE source_node_id = ?1 AND source_epoch = ?2 AND stream = ?3
                   AND subject_node_id = ?4 AND observer_node_id = ?5
                   AND schema_id = ?6 AND schema_version = ?7 AND record_key = ?8
                   AND is_tombstone = 1",
                params![
                    tombstone.source_node_id,
                    durable_i64(tombstone.source_epoch, "source epoch")?,
                    tombstone.stream,
                    tombstone.subject_node_id,
                    tombstone.observer_node_id,
                    tombstone.schema_id,
                    i64::from(tombstone.schema_version),
                    tombstone.record_key,
                ],
            )
            .map_err(sqlite_error)?;
        maintain_sqlite(connection)
    }

    pub(crate) fn repository_history_records(
        &self,
        subject_node_id: Option<&str>,
        start_unix_seconds: Option<u64>,
        end_unix_seconds: Option<u64>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<RepositoryHistoryRecordRow>> {
        self.repository_history_records_for_schema(
            subject_node_id,
            None,
            start_unix_seconds,
            end_unix_seconds,
            offset,
            limit,
        )
    }

    pub(crate) fn repository_history_records_for_schema(
        &self,
        subject_node_id: Option<&str>,
        schema_id: Option<&str>,
        start_unix_seconds: Option<u64>,
        end_unix_seconds: Option<u64>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<RepositoryHistoryRecordRow>> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(Vec::new());
        };
        let mut statement = connection
            .prepare(
                "
                SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
                       observer_node_id, schema_id, schema_version, record_key, is_tombstone,
                       observed_start, observed_end, received_at, payload
                FROM repository_history_records
                WHERE is_tombstone = 0
                  AND (?1 IS NULL OR subject_node_id = ?1)
                  AND (?2 IS NULL OR schema_id = ?2)
                  AND (?3 IS NULL OR observed_end >= ?3)
                  AND (?4 IS NULL OR observed_start <= ?4)
                ORDER BY observed_start, source_node_id, source_epoch, stream, sequence
                LIMIT ?5 OFFSET ?6
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                params![
                    subject_node_id,
                    schema_id,
                    start_unix_seconds.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                    end_unix_seconds.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                    i64::try_from(limit).unwrap_or(i64::MAX),
                    i64::try_from(offset).unwrap_or(i64::MAX),
                ],
                repository_history_record_row,
            )
            .map_err(sqlite_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)
    }

    pub(crate) fn repository_history_coverage(
        &self,
        subject_node_id: Option<&str>,
    ) -> Result<Option<super::RepositoryHistoryCoverage>> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(None);
        };
        connection
            .query_row(
                "
                SELECT MIN(observed_start), MAX(observed_end), MIN(received_at), MAX(received_at)
                FROM repository_history_records
                WHERE is_tombstone = 0
                  AND (?1 IS NULL OR subject_node_id = ?1)
                ",
                [subject_node_id],
                |row| {
                    let observed_start = row.get::<_, Option<i64>>(0)?;
                    let Some(observed_start) = observed_start else {
                        return Ok(None);
                    };
                    Ok(Some(RepositoryHistoryCoverage {
                        observed_start_unix_seconds: u64::try_from(observed_start)
                            .unwrap_or(u64::MAX),
                        observed_end_unix_seconds: u64::try_from(row.get::<_, i64>(1)?)
                            .unwrap_or(u64::MAX),
                        received_start_unix_seconds: u64::try_from(row.get::<_, i64>(2)?)
                            .unwrap_or(u64::MAX),
                        received_end_unix_seconds: u64::try_from(row.get::<_, i64>(3)?)
                            .unwrap_or(u64::MAX),
                    }))
                },
            )
            .map_err(sqlite_error)
    }

    pub(crate) fn repository_history_incomplete_aggregate_range_for_schema(
        &self,
        subject_node_id: Option<&str>,
        schema_id: Option<&str>,
        start_unix_seconds: u64,
        end_unix_seconds: u64,
    ) -> Result<Option<(u64, u64)>> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(None);
        };
        connection
            .query_row(
                "
                SELECT MIN(COALESCE(aggregate_start, observed_start)),
                       MAX(COALESCE(aggregate_end, observed_end))
                FROM repository_history_records
                WHERE is_tombstone = 0
                  AND (?1 IS NULL OR subject_node_id = ?1)
                  AND (?2 IS NULL OR schema_id = ?2)
                  AND (aggregate_complete = 0 OR aggregate_complete IS NULL)
                  AND COALESCE(aggregate_start, observed_start) <= ?3
                  AND COALESCE(aggregate_end, observed_end) >= ?4
                ",
                params![
                    subject_node_id,
                    schema_id,
                    i64::try_from(end_unix_seconds).unwrap_or(i64::MAX),
                    i64::try_from(start_unix_seconds).unwrap_or(i64::MAX),
                ],
                |row| {
                    let Some(start) = row.get::<_, Option<i64>>(0)? else {
                        return Ok(None);
                    };
                    Ok(Some((
                        u64::try_from(start).unwrap_or(u64::MAX),
                        u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
                    )))
                },
            )
            .map_err(sqlite_error)
    }

    pub(crate) fn clear_repository_history(&self) -> Result<()> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(());
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        transaction
            .execute("DELETE FROM repository_history_records", [])
            .map_err(sqlite_error)?;
        transaction
            .execute("DELETE FROM repository_history_segments", [])
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)?;
        maintain_sqlite(connection)
    }
}

fn replace_repository_history_records_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    removed: &[RepositoryHistoryRecordRow],
    retained: &[RepositoryHistoryRecordRow],
) -> Result<()> {
    for row in removed {
        transaction
            .execute(
                "DELETE FROM repository_history_records
                 WHERE source_node_id = ?1 AND source_epoch = ?2 AND stream = ?3
                   AND sequence = ?4",
                params![
                    row.source_node_id,
                    durable_i64(row.source_epoch, "source epoch")?,
                    row.stream,
                    durable_i64(row.sequence, "sequence")?,
                ],
            )
            .map_err(sqlite_error)?;
    }
    for row in retained {
        super::upsert_repository_history_record(transaction, row)?;
    }
    Ok(())
}
