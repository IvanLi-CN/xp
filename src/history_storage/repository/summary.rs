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

impl HistoryStorage {
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
        let mut statement = connection
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
        let rows = statement
            .query_map(
                params![
                    source_node_id,
                    source_epoch_i64,
                    stream,
                    durable_i64(first, "summary block first")?,
                    durable_i64(last, "summary block last")?
                ],
                |row| {
                    Ok((
                        u64::try_from(row.get::<_, i64>(3)?).unwrap_or(u64::MAX),
                        row.get::<_, String>(0)?,
                        u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        u32::try_from(row.get::<_, i64>(7)?).unwrap_or(u32::MAX),
                        row.get::<_, Vec<u8>>(8)?,
                        u64::try_from(row.get::<_, i64>(9)?).unwrap_or(u64::MAX),
                        row.get::<_, Vec<u8>>(10)?,
                    ))
                },
            )
            .map_err(sqlite_error)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(sqlite_error)?);
        }
        drop(statement);
        let transaction = connection.transaction().map_err(sqlite_error)?;
        if records.is_empty() {
            transaction
                .execute(
                    "DELETE FROM repository_history_sequence_summary_blocks
                      WHERE source_node_id = ?1 AND source_epoch = ?2 AND stream = ?3
                        AND block_index = ?4",
                    params![source_node_id, source_epoch_i64, stream, block_index_i64],
                )
                .map_err(sqlite_error)?;
        } else {
            let mut root = [0u8; 32];
            for (
                sequence,
                source,
                epoch,
                stream_name,
                subject,
                observer,
                schema,
                version,
                key,
                observed,
                payload,
            ) in &records
            {
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
            }
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
                        durable_i64(
                            records.first().map(|row| row.0).unwrap_or(first),
                            "summary first"
                        )?,
                        durable_i64(
                            records.last().map(|row| row.0).unwrap_or(last),
                            "summary last"
                        )?,
                        i64::try_from(records.len()).unwrap_or(i64::MAX),
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
                    source_epoch: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
                    stream: row.get(2)?,
                    block_index: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(u64::MAX),
                    first_sequence: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(u64::MAX),
                    last_sequence: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(u64::MAX),
                    record_count: u64::try_from(row.get::<_, i64>(6)?).unwrap_or(u64::MAX),
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
