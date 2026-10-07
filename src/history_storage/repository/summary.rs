use super::*;
use sha2::Digest as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepositoryHistorySequenceSummaryBlock {
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
    pub(crate) block_index: u64,
    pub(crate) first_sequence: u64,
    pub(crate) last_sequence: u64,
    pub(crate) record_count: u64,
    pub(crate) digest: [u8; 32],
}
type SequenceSummaryMigrationPage = (
    Vec<(String, u64, String, u64)>,
    Option<RepositoryHistoryCompactionCursor>,
    bool,
);

impl HistoryStorage {
    pub(crate) fn repository_history_sequence_summary_migration_page(
        &self,
        after: Option<&RepositoryHistoryCompactionCursor>,
        limit: usize,
    ) -> Result<SequenceSummaryMigrationPage> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok((Vec::new(), None, true));
        };
        let after = after.cloned().unwrap_or(RepositoryHistoryCompactionCursor {
            observed_start_unix_seconds: 0,
            source_node_id: String::new(),
            source_epoch: 0,
            stream: String::new(),
            sequence: 0,
        });
        let mut statement = connection
            .prepare(
                "SELECT source_node_id, source_epoch, stream, sequence
                   FROM repository_history_records
                  WHERE is_tombstone = 0
                    AND (source_node_id, source_epoch, stream, sequence)
                        > (?1, ?2, ?3, ?4)
                  ORDER BY source_node_id, source_epoch, stream, sequence
                  LIMIT ?5",
            )
            .map_err(sqlite_error)?;
        let mut rows = statement
            .query(params![
                after.source_node_id,
                durable_i64(after.source_epoch, "summary migration epoch")?,
                after.stream,
                durable_i64(after.sequence, "summary migration sequence")?,
                durable_i64(limit as u64, "summary migration limit")?
            ])
            .map_err(sqlite_error)?;
        let mut blocks = Vec::new();
        let mut row_count = 0usize;
        let mut last = None;
        while let Some(row) = rows.next().map_err(sqlite_error)? {
            let source = row.get::<_, String>(0).map_err(sqlite_error)?;
            let epoch = checked_u64(row.get::<_, i64>(1).map_err(sqlite_error)?, 1)
                .map_err(sqlite_error)?;
            let stream = row.get::<_, String>(2).map_err(sqlite_error)?;
            let sequence = checked_u64(row.get::<_, i64>(3).map_err(sqlite_error)?, 3)
                .map_err(sqlite_error)?;
            row_count = row_count.saturating_add(1);
            blocks.push((source.clone(), epoch, stream.clone(), sequence / 4096));
            last = Some(RepositoryHistoryCompactionCursor {
                observed_start_unix_seconds: 0,
                source_node_id: source,
                source_epoch: epoch,
                stream,
                sequence,
            });
        }
        drop(rows);
        drop(statement);
        blocks.sort();
        blocks.dedup();
        Ok((blocks, last, row_count < limit))
    }

    pub(crate) fn mark_repository_history_sequence_summary_blocks_dirty(
        &self,
        blocks: &[(String, u64, String, u64)],
    ) -> Result<()> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(());
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        for (source_node_id, source_epoch, stream, block_index) in blocks {
            transaction
                .execute(
                    "INSERT INTO repository_history_sequence_summary_blocks
                       (source_node_id, source_epoch, stream, block_index, first_sequence,
                        last_sequence, record_count, digest, dirty)
                     VALUES (?1, ?2, ?3, ?4, 0, 0, 0, zeroblob(32), 1)
                     ON CONFLICT(source_node_id, source_epoch, stream, block_index)
                     DO UPDATE SET dirty = 1",
                    params![
                        source_node_id,
                        durable_i64(*source_epoch, "summary seed source epoch")?,
                        stream,
                        durable_i64(*block_index, "summary seed block index")?
                    ],
                )
                .map_err(sqlite_error)?;
        }
        transaction.commit().map_err(sqlite_error)
    }

    pub(crate) fn repository_history_dirty_sequence_summary_blocks(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, u64, String, u64)>> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(Vec::new());
        };
        let mut statement = connection
            .prepare(
                "SELECT source_node_id, source_epoch, stream, block_index
                   FROM repository_history_sequence_summary_blocks
                  WHERE dirty = 1
                  ORDER BY source_node_id, source_epoch, stream, block_index
                  LIMIT ?1",
            )
            .map_err(sqlite_error)?;
        statement
            .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
                Ok((
                    row.get(0)?,
                    u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
                    row.get(2)?,
                    u64::try_from(row.get::<_, i64>(3)?).unwrap_or(u64::MAX),
                ))
            })
            .map_err(sqlite_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)
    }

    pub(crate) fn rebuild_repository_history_sequence_summary_block(
        &self,
        source_node_id: &str,
        source_epoch: u64,
        stream: &str,
        block_index: u64,
    ) -> Result<()> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(());
        };
        let source_epoch_i64 = durable_i64(source_epoch, "summary block source epoch")?;
        let block_index_i64 = durable_i64(block_index, "summary block index")?;
        let first = block_index.saturating_mul(4096);
        let last = first.saturating_add(4095);
        let transaction = connection.transaction().map_err(sqlite_error)?;
        let mut statement = transaction
            .prepare(
                "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
                        observer_node_id, schema_id, schema_version, record_key,
                        observed_start, payload
                   FROM repository_history_records
                  WHERE is_tombstone = 0
                    AND source_node_id = ?1 AND source_epoch = ?2 AND stream = ?3
                    AND sequence BETWEEN ?4 AND ?5
                  ORDER BY sequence",
            )
            .map_err(sqlite_error)?;
        let mut rows = statement
            .query(params![
                source_node_id,
                source_epoch_i64,
                stream,
                durable_i64(first, "summary block first")?,
                durable_i64(last, "summary block last")?
            ])
            .map_err(sqlite_error)?;
        let mut root = [0u8; 32];
        let mut first_sequence = None;
        let mut last_sequence = None;
        let mut record_count = 0u64;
        while let Some(row) = rows.next().map_err(sqlite_error)? {
            let sequence = checked_u64(row.get::<_, i64>(3).map_err(sqlite_error)?, 3)
                .map_err(sqlite_error)?;
            if !(first..=last).contains(&sequence)
                || last_sequence.is_some_and(|previous| sequence <= previous)
            {
                return Err(HistoryStorageError(
                    "invalid sequence summary row ordering".to_owned(),
                ));
            }
            let source = row.get::<_, String>(0).map_err(sqlite_error)?;
            let epoch = checked_u64(row.get::<_, i64>(1).map_err(sqlite_error)?, 1)
                .map_err(sqlite_error)?;
            let stream_name = row.get::<_, String>(2).map_err(sqlite_error)?;
            let subject = row.get::<_, String>(4).map_err(sqlite_error)?;
            let observer = row.get::<_, String>(5).map_err(sqlite_error)?;
            let schema = row.get::<_, String>(6).map_err(sqlite_error)?;
            let version = checked_u32(row.get::<_, i64>(7).map_err(sqlite_error)?, 7)
                .map_err(sqlite_error)?;
            let key = row.get::<_, Vec<u8>>(8).map_err(sqlite_error)?;
            let observed = checked_u64(row.get::<_, i64>(9).map_err(sqlite_error)?, 9)
                .map_err(sqlite_error)?;
            let payload = row.get::<_, Vec<u8>>(10).map_err(sqlite_error)?;
            let mut leaf = sha2::Sha256::new();
            leaf.update(b"xp-history-repository-sequence-leaf-v2\0");
            leaf.update(source.as_bytes());
            leaf.update(epoch.to_be_bytes());
            leaf.update(stream_name.as_bytes());
            leaf.update(sequence.to_be_bytes());
            leaf.update(subject.as_bytes());
            leaf.update(observer.as_bytes());
            leaf.update(schema.as_bytes());
            leaf.update(version.to_be_bytes());
            leaf.update(key);
            leaf.update(observed.to_be_bytes());
            leaf.update(payload);
            let leaf: [u8; 32] = leaf.finalize().into();
            let mut node = sha2::Sha256::new();
            node.update(b"xp-history-repository-sequence-node-v2\0");
            node.update(root);
            node.update(leaf);
            root = node.finalize().into();
            first_sequence.get_or_insert(sequence);
            last_sequence = Some(sequence);
            record_count = record_count.saturating_add(1);
        }
        drop(rows);
        drop(statement);
        if record_count == 0 {
            transaction
                .execute(
                    "DELETE FROM repository_history_sequence_summary_blocks
                      WHERE source_node_id = ?1 AND source_epoch = ?2 AND stream = ?3
                        AND block_index = ?4",
                    params![source_node_id, source_epoch_i64, stream, block_index_i64],
                )
                .map_err(sqlite_error)?;
        } else {
            transaction
                .execute(
                    "INSERT INTO repository_history_sequence_summary_blocks
                       (source_node_id, source_epoch, stream, block_index, first_sequence,
                        last_sequence, record_count, digest, dirty)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)
                     ON CONFLICT(source_node_id, source_epoch, stream, block_index)
                     DO UPDATE SET first_sequence = excluded.first_sequence,
                       last_sequence = excluded.last_sequence, record_count = excluded.record_count,
                       digest = excluded.digest, dirty = 0",
                    params![
                        source_node_id,
                        source_epoch_i64,
                        stream,
                        block_index_i64,
                        durable_i64(first_sequence.unwrap_or(first), "summary first")?,
                        durable_i64(last_sequence.unwrap_or(last), "summary last")?,
                        i64::try_from(record_count).unwrap_or(i64::MAX),
                        root.to_vec(),
                    ],
                )
                .map_err(sqlite_error)?;
        }
        transaction.commit().map_err(sqlite_error)
    }

    pub(crate) fn repository_history_sequence_summary_blocks(
        &self,
    ) -> Result<Vec<RepositoryHistorySequenceSummaryBlock>> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(Vec::new());
        };
        let mut statement = connection
            .prepare(
                "SELECT source_node_id, source_epoch, stream, block_index, first_sequence,
                        last_sequence, record_count, digest
                   FROM repository_history_sequence_summary_blocks
                  WHERE dirty = 0
                  ORDER BY source_node_id, source_epoch, stream, block_index",
            )
            .map_err(sqlite_error)?;
        statement
            .query_map([], |row| {
                let digest = row.get::<_, Vec<u8>>(7)?;
                let digest: [u8; 32] = digest.try_into().map_err(|_| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Blob,
                        "summary digest must be 32 bytes".into(),
                    )
                })?;
                Ok(RepositoryHistorySequenceSummaryBlock {
                    source_node_id: row.get(0)?,
                    source_epoch: checked_u64(row.get::<_, i64>(1)?, 1)?,
                    stream: row.get(2)?,
                    block_index: checked_u64(row.get::<_, i64>(3)?, 3)?,
                    first_sequence: checked_u64(row.get::<_, i64>(4)?, 4)?,
                    last_sequence: checked_u64(row.get::<_, i64>(5)?, 5)?,
                    record_count: checked_u64(row.get::<_, i64>(6)?, 6)?,
                    digest,
                })
            })
            .map_err(sqlite_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)
    }
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

fn checked_u64(value: i64, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            "summary integer must be nonnegative".into(),
        )
    })
}

fn checked_u32(value: i64, column: usize) -> rusqlite::Result<u32> {
    u32::try_from(value).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            "summary integer is out of range".into(),
        )
    })
}
