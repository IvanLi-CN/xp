use crate::state::history_storage::HistoryStorageDiagnosticOperation;

use super::*;

impl HistoryStorage {
    pub(crate) fn repository_history_record_count(&self) -> Result<usize> {
        self.repository_history_count(
            "repository_history_records",
            "history_storage.repository_history_count",
        )
    }

    pub(crate) fn repository_history_record_count_with_caller(
        &self,
        caller_class: &'static str,
    ) -> Result<usize> {
        self.repository_history_count("repository_history_records", caller_class)
    }

    pub(crate) fn repository_history_has_expired_records(
        &self,
        end_unix_seconds: u64,
    ) -> Result<bool> {
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::RetentionExpiredRecordProbe,
            "history_storage.retention",
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish_with_count(0);
            return Ok(false);
        };
        let value = connection
            .query_row(
                "SELECT 1 FROM repository_history_records
                 INDEXED BY repository_history_records_export_filter
                 WHERE is_tombstone = 0 AND observed_end < ?1 LIMIT 1",
                [i64::try_from(end_unix_seconds).unwrap_or(i64::MAX)],
                |_| Ok(()),
            )
            .optional()
            .map(|row| row.is_some())
            .map_err(sqlite_error)?;
        diagnostic.finish_with_count(if value { 1 } else { 0 });
        Ok(value)
    }

    #[cfg(test)]
    pub(crate) fn repository_history_segment_count(&self) -> Result<usize> {
        self.repository_history_count(
            "repository_history_segments",
            "history_storage.repository_history_count",
        )
    }

    pub(crate) fn repository_history_segment_count_with_caller(
        &self,
        caller_class: &'static str,
    ) -> Result<usize> {
        self.repository_history_count("repository_history_segments", caller_class)
    }

    pub(crate) fn replace_repository_history_records_and_prune(
        &self,
        removed: &[RepositoryHistoryRecordRow],
        retained: &[RepositoryHistoryRecordRow],
        record_end_unix_seconds: u64,
        segment_closed_at_unix_seconds: u64,
        control_payload: &[u8],
    ) -> Result<RepositoryCommitOutcome> {
        #[cfg(test)]
        let fail_maintenance = self
            .fail_history_rewrite_maintenance
            .load(std::sync::atomic::Ordering::Relaxed);
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::RetentionReplaceAndPrune,
            "history_storage.retention",
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Err(HistoryStorageError(
                "repository history row storage requires SQLite".to_owned(),
            ));
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        replace_repository_history_records_in_transaction(&transaction, removed, retained)?;
        transaction
            .execute(
                "DELETE FROM repository_history_records
                 WHERE observed_end < ?1 AND is_tombstone = 0",
                [i64::try_from(record_end_unix_seconds).unwrap_or(i64::MAX)],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "DELETE FROM repository_history_segments WHERE closed_at < ?1",
                [i64::try_from(segment_closed_at_unix_seconds).unwrap_or(i64::MAX)],
            )
            .map_err(sqlite_error)?;
        write_snapshot(&transaction, REPOSITORY_REPLICA_KEY, control_payload)?;
        transaction.commit().map_err(sqlite_error)?;
        #[cfg(test)]
        let maintenance_result = if fail_maintenance {
            Err(HistoryStorageError(
                "injected history rewrite maintenance failure".to_owned(),
            ))
        } else {
            maintain_sqlite(connection)
        };
        #[cfg(not(test))]
        let maintenance_result = maintain_sqlite(connection);
        let outcome = RepositoryCommitOutcome {
            maintenance_degraded: finish_post_commit_maintenance(maintenance_result),
        };
        diagnostic.finish_with_count(removed.len().saturating_add(retained.len()));
        Ok(outcome)
    }

    pub(crate) fn repository_history_records_for_compaction(
        &self,
        end_unix_seconds: u64,
        after: Option<&RepositoryHistoryCompactionCursor>,
        limit: usize,
    ) -> Result<Vec<RepositoryHistoryRecordRow>> {
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::RetentionCompactionPage,
            "history_storage.retention",
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish_with_count(0);
            return Ok(Vec::new());
        };
        let after = after.cloned().unwrap_or(RepositoryHistoryCompactionCursor {
            observed_start_unix_seconds: 0,
            source_node_id: String::new(),
            source_epoch: 0,
            stream: String::new(),
            sequence: 0,
        });
        let after_source_epoch = durable_i64(after.source_epoch, "cursor source epoch")?;
        let after_sequence = durable_i64(after.sequence, "cursor sequence")?;
        let mut statement = connection
            .prepare(
                "
                SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
                       observer_node_id, schema_id, schema_version, record_key, is_tombstone,
                       observed_start, observed_end, received_at, payload,
                       aggregate_complete, aggregate_start, aggregate_end
                FROM repository_history_records INDEXED BY repository_history_records_keyset
                WHERE is_tombstone = 0 AND observed_start < ?1
                  AND (observed_start, source_node_id, source_epoch, stream, sequence)
                      > (?2, ?3, ?4, ?5, ?6)
                ORDER BY observed_start, source_node_id, source_epoch, stream, sequence
                LIMIT ?7
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                params![
                    i64::try_from(end_unix_seconds).unwrap_or(i64::MAX),
                    i64::try_from(after.observed_start_unix_seconds).unwrap_or(i64::MAX),
                    after.source_node_id,
                    after_source_epoch,
                    after.stream,
                    after_sequence,
                    i64::try_from(limit).unwrap_or(i64::MAX),
                ],
                |row| {
                    let mut record = repository_history_record_row(row)?;
                    let complete = sqlite_nullable_integer(row.get_ref(14)?);
                    let start = sqlite_nullable_integer(row.get_ref(15)?);
                    let end = sqlite_nullable_integer(row.get_ref(16)?);
                    let range_valid = match (start, end) {
                        (Some(None), Some(None)) => true,
                        (Some(Some(start)), Some(Some(end))) => start >= 0 && start <= end,
                        _ => false,
                    };
                    let metadata_valid =
                        matches!(complete, Some(None | Some(0 | 1))) && range_valid;
                    record.aggregate_complete = if metadata_valid {
                        complete.flatten().map(|value| value != 0)
                    } else {
                        None
                    };
                    record.aggregate_start_unix_seconds =
                        start.flatten().and_then(|value| u64::try_from(value).ok());
                    record.aggregate_end_unix_seconds =
                        end.flatten().and_then(|value| u64::try_from(value).ok());
                    Ok(record)
                },
            )
            .map_err(sqlite_error)?;
        let rows = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)?;
        diagnostic.finish_with_count(rows.len());
        Ok(rows)
    }

    /// Bounded keyset export of the retained canonical representation. This is used only by a
    /// syncing repository after the short signed-frame repair cache has expired.
    pub(crate) fn repository_history_records_page(
        &self,
        after: Option<&RepositoryHistoryCompactionCursor>,
        high_watermark: &RepositoryHistoryCompactionCursor,
        limit: usize,
        tombstones_only: bool,
        repair_cache_cutoff_unix_seconds: u64,
        received_at_cutoff_unix_seconds: u64,
    ) -> Result<Vec<RepositoryHistoryRecordRow>> {
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::TieredBackfillRecordsPage,
            "history_storage.tiered_backfill",
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish_with_count(0);
            return Ok(Vec::new());
        };
        let after = after.cloned().unwrap_or(RepositoryHistoryCompactionCursor {
            observed_start_unix_seconds: 0,
            source_node_id: String::new(),
            source_epoch: 0,
            stream: String::new(),
            sequence: 0,
        });
        let after_source_epoch = durable_i64(after.source_epoch, "cursor source epoch")?;
        let after_sequence = durable_i64(after.sequence, "cursor sequence")?;
        let watermark_source_epoch =
            durable_i64(high_watermark.source_epoch, "watermark source epoch")?;
        let watermark_sequence = durable_i64(high_watermark.sequence, "watermark sequence")?;
        let mut statement = connection
            .prepare(
                "
                SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
                       observer_node_id, schema_id, schema_version, record_key, is_tombstone,
                       observed_start, observed_end, received_at, payload
                FROM repository_history_records INDEXED BY repository_history_records_keyset
                WHERE is_tombstone = ?1
                  AND (is_tombstone = 1 OR observed_end < ?2)
                  AND received_at <= ?3
                  AND (observed_start, source_node_id, source_epoch, stream, sequence)
                      > (?4, ?5, ?6, ?7, ?8)
                  AND (observed_start, source_node_id, source_epoch, stream, sequence)
                      <= (?9, ?10, ?11, ?12, ?13)
                ORDER BY observed_start, source_node_id, source_epoch, stream, sequence
                LIMIT ?14
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                params![
                    tombstones_only,
                    i64::try_from(repair_cache_cutoff_unix_seconds).unwrap_or(i64::MAX),
                    i64::try_from(received_at_cutoff_unix_seconds).unwrap_or(i64::MAX),
                    i64::try_from(after.observed_start_unix_seconds).unwrap_or(i64::MAX),
                    after.source_node_id,
                    after_source_epoch,
                    after.stream,
                    after_sequence,
                    i64::try_from(high_watermark.observed_start_unix_seconds).unwrap_or(i64::MAX),
                    high_watermark.source_node_id.as_str(),
                    watermark_source_epoch,
                    high_watermark.stream.as_str(),
                    watermark_sequence,
                    i64::try_from(limit).unwrap_or(i64::MAX),
                ],
                repository_history_record_row,
            )
            .map_err(sqlite_error)?;
        let rows = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)?;
        diagnostic.finish_with_count(rows.len());
        Ok(rows)
    }

    pub(crate) fn repository_history_export_watermarks(
        &self,
        repair_cache_cutoff_unix_seconds: u64,
    ) -> Result<Option<RepositoryHistoryExportWatermarks>> {
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::TieredBackfillExportWatermarks,
            "history_storage.tiered_backfill",
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish();
            return Ok(None);
        };
        let received_at_cutoff = {
            let diagnostic = self.begin_diagnostic(
                HistoryStorageDiagnosticOperation::TieredBackfillReceivedAtCutoff,
                "history_storage.tiered_backfill",
            );
            let value = connection
                .query_row(
                    "SELECT MAX(received_at)
                     FROM repository_history_records
                    WHERE (is_tombstone = 1 OR observed_end < ?1)",
                    [i64::try_from(repair_cache_cutoff_unix_seconds).unwrap_or(i64::MAX)],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .map_err(sqlite_error)?;
            diagnostic.finish();
            value.map(|value| u64::try_from(value).unwrap_or(u64::MAX))
        };
        let Some(received_at_cutoff) = received_at_cutoff else {
            diagnostic.finish();
            return Ok(None);
        };
        let watermark_for = |tombstones_only: bool| {
            let operation = if tombstones_only {
                HistoryStorageDiagnosticOperation::TieredBackfillTombstoneWatermark
            } else {
                HistoryStorageDiagnosticOperation::TieredBackfillRecordWatermark
            };
            let diagnostic = self.begin_diagnostic(operation, "history_storage.tiered_backfill");
            let value = connection
                .query_row(
                    "SELECT source_node_id, source_epoch, stream, sequence,
                        observed_start
                 FROM repository_history_records
                 WHERE is_tombstone = ?1
                   AND (is_tombstone = 1 OR observed_end < ?2)
                   AND received_at <= ?3
                 ORDER BY observed_start DESC, source_node_id DESC, source_epoch DESC,
                          stream DESC, sequence DESC
                 LIMIT 1",
                    params![
                        tombstones_only,
                        i64::try_from(repair_cache_cutoff_unix_seconds).unwrap_or(i64::MAX),
                        i64::try_from(received_at_cutoff).unwrap_or(i64::MAX),
                    ],
                    |row| {
                        Ok(RepositoryHistoryCompactionCursor {
                            observed_start_unix_seconds: u64::try_from(row.get::<_, i64>(4)?)
                                .unwrap_or(u64::MAX),
                            source_node_id: row.get(0)?,
                            source_epoch: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
                            stream: row.get(2)?,
                            sequence: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(u64::MAX),
                        })
                    },
                )
                .optional()
                .map_err(sqlite_error)?;
            diagnostic.finish_with_count(if value.is_some() { 1 } else { 0 });
            Ok(value)
        };
        let tombstone_watermark = watermark_for(true)?;
        let record_watermark = watermark_for(false)?;
        diagnostic.finish();
        Ok(
            (tombstone_watermark.is_some() || record_watermark.is_some()).then_some((
                tombstone_watermark,
                record_watermark,
                received_at_cutoff,
            )),
        )
    }

    pub(crate) fn refresh_repository_history_export(
        &self,
        session_id: &str,
        now_unix_seconds: u64,
    ) -> Result<()> {
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::TieredBackfillExportRefresh,
            "history_storage.tiered_backfill",
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish();
            return Ok(());
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        transaction
            .execute(
                "DELETE FROM repository_history_export_leases WHERE expires_at <= ?1",
                [i64::try_from(now_unix_seconds).unwrap_or(i64::MAX)],
            )
            .map_err(sqlite_error)?;
        let existing = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM repository_history_export_leases WHERE session_id = ?1
                )",
                [session_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(sqlite_error)?;
        if !existing {
            let count_diagnostic = self.begin_diagnostic(
                HistoryStorageDiagnosticOperation::TieredBackfillExportActiveCount,
                "history_storage.tiered_backfill",
            );
            let active = transaction
                .query_row(
                    "SELECT COUNT(*) FROM repository_history_export_leases",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(sqlite_error)?;
            count_diagnostic.finish();
            if usize::try_from(active).unwrap_or(usize::MAX)
                >= MAX_ACTIVE_REPOSITORY_HISTORY_EXPORTS
            {
                return Err(HistoryStorageError(
                    "repository history export session limit reached".to_owned(),
                ));
            }
        }
        let upsert_diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::TieredBackfillExportUpsert,
            "history_storage.tiered_backfill",
        );
        transaction
            .execute(
                "INSERT INTO repository_history_export_leases (session_id, expires_at)
                 VALUES (?1, ?2)
                 ON CONFLICT(session_id) DO UPDATE SET expires_at = excluded.expires_at",
                params![
                    session_id,
                    i64::try_from(
                        now_unix_seconds.saturating_add(REPOSITORY_HISTORY_EXPORT_LEASE_SECONDS)
                    )
                    .unwrap_or(i64::MAX),
                ],
            )
            .map_err(sqlite_error)?;
        upsert_diagnostic.finish();
        transaction.commit().map_err(sqlite_error)?;
        diagnostic.finish();
        Ok(())
    }

    pub(crate) fn finish_repository_history_export(&self, session_id: &str) -> Result<()> {
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::TieredBackfillExportFinish,
            "history_storage.tiered_backfill",
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish();
            return Ok(());
        };
        connection
            .execute(
                "DELETE FROM repository_history_export_leases WHERE session_id = ?1",
                [session_id],
            )
            .map_err(sqlite_error)?;
        diagnostic.finish();
        Ok(())
    }

    pub(crate) fn has_active_repository_history_export(
        &self,
        now_unix_seconds: u64,
    ) -> Result<bool> {
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::RetentionActiveExport,
            "history_storage.retention",
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish_with_count(0);
            return Ok(false);
        };
        connection
            .execute(
                "DELETE FROM repository_history_export_leases WHERE expires_at <= ?1",
                [i64::try_from(now_unix_seconds).unwrap_or(i64::MAX)],
            )
            .map_err(sqlite_error)?;
        let value = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM repository_history_export_leases)",
                [],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        diagnostic.finish_with_count(if value { 1 } else { 0 });
        Ok(value)
    }

    pub(crate) fn has_repository_history_export_session(
        &self,
        session_id: &str,
        now_unix_seconds: u64,
    ) -> Result<bool> {
        let diagnostic = self.begin_diagnostic(
            HistoryStorageDiagnosticOperation::TieredBackfillExportSession,
            "history_storage.tiered_backfill",
        );
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish_with_count(0);
            return Ok(false);
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        transaction
            .execute(
                "DELETE FROM repository_history_export_leases WHERE expires_at <= ?1",
                [i64::try_from(now_unix_seconds).unwrap_or(i64::MAX)],
            )
            .map_err(sqlite_error)?;
        let active = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM repository_history_export_leases WHERE session_id = ?1
                )",
                [session_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)?;
        diagnostic.finish_with_count(if active { 1 } else { 0 });
        Ok(active)
    }

    fn repository_history_count(&self, table: &str, caller_class: &'static str) -> Result<usize> {
        let operation = match table {
            "repository_history_records" => {
                HistoryStorageDiagnosticOperation::RuntimeStatusRecordCount
            }
            "repository_history_segments" => {
                HistoryStorageDiagnosticOperation::RuntimeStatusSegmentCount
            }
            _ => {
                return Err(HistoryStorageError(
                    "unknown repository history table".to_owned(),
                ));
            }
        };
        let diagnostic = self.begin_diagnostic(operation, caller_class);
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            diagnostic.finish_with_count(0);
            return Ok(0);
        };
        let query = match table {
            // The keyset index contains the complete ordering metadata but no payload. Counting
            // through it keeps startup metadata-only even when history rows carry large values.
            "repository_history_records" => {
                "SELECT COUNT(source_node_id) FROM repository_history_records
                 INDEXED BY repository_history_records_keyset"
            }
            "repository_history_segments" => {
                "SELECT COUNT(id) FROM repository_history_segments
                 INDEXED BY repository_history_segments_sync_order_v2"
            }
            _ => unreachable!("repository history count operation was validated above"),
        };
        let value = connection
            .query_row(query, [], |row| row.get::<_, i64>(0))
            .map(|value| usize::try_from(value).unwrap_or(usize::MAX))
            .map_err(sqlite_error)?;
        diagnostic.finish_with_count(value);
        Ok(value)
    }
}
