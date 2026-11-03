use super::*;

// Bound non-wire allocations too, including IDs supplied by a repair checkpoint.
const MAX_ID_BYTES: usize = 1024;
const MAX_METADATA_BYTES: usize = SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES;

impl HistoryStorage {
    /// Read every requested journal row in caller order, including legacy unordered rows.
    /// IDs must be unique. Reject missing IDs and requests exceeding 256 rows or the
    /// smaller of `max_wire_bytes` and 1 MiB; never return a truncated page.
    /// Metadata is independently capped at 1 MiB and IDs at 1024 bytes each.
    /// All size/existence checks precede payload reads in one SQLite read snapshot.
    pub(crate) fn source_delivery_journal_by_ids(
        &self,
        ids: &[String],
        max_wire_bytes: usize,
    ) -> Result<Vec<SourceDeliveryJournalRow>> {
        if ids.len() > SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS {
            return Err(HistoryStorageError(
                "journal exact lookup count exceeds limit".into(),
            ));
        }
        let mut unique = std::collections::BTreeSet::new();
        for id in ids {
            if id.len() > MAX_ID_BYTES || !unique.insert(id.as_str()) {
                return Err(HistoryStorageError(
                    "journal exact lookup invalid IDs".into(),
                ));
            }
        }
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Err(HistoryStorageError(
                "source delivery journal requires SQLite".into(),
            ));
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        let wire_limit = max_wire_bytes.min(SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES);
        let mut wire_bytes = 0_usize;
        let mut metadata_bytes = 0_usize;
        {
            let mut sizes = transaction
                .prepare(
                    "SELECT length(wire), length(identity),
                            length(CAST(id AS BLOB)), length(CAST(stream AS BLOB))
                     FROM source_delivery_journal
                     WHERE id = ?1 AND typeof(wire) = 'blob' AND typeof(identity) = 'blob'",
                )
                .map_err(sqlite_error)?;
            for id in ids {
                let lengths = sizes
                    .query_row([id], |row| {
                        Ok([row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get(3)?])
                    })
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| {
                        HistoryStorageError("journal exact lookup missing or invalid row".into())
                    })?;
                for (index, length) in lengths.into_iter().enumerate() {
                    let length = usize::try_from(length)
                        .map_err(|_| HistoryStorageError("invalid journal field length".into()))?;
                    let (total, limit) = if index == 0 {
                        (&mut wire_bytes, wire_limit)
                    } else {
                        (&mut metadata_bytes, MAX_METADATA_BYTES)
                    };
                    if length > limit.saturating_sub(*total) {
                        return Err(HistoryStorageError(
                            "journal exact lookup exceeds byte budget".into(),
                        ));
                    }
                    *total += length;
                }
            }
        }
        let mut result = Vec::with_capacity(ids.len());
        {
            let mut rows = transaction
                .prepare(
                    "SELECT id, stream, closed_at, identity, wire
                     FROM source_delivery_journal WHERE id = ?1",
                )
                .map_err(sqlite_error)?;
            for id in ids {
                result.push(
                    rows.query_row([id], source_delivery_journal_row)
                        .map_err(sqlite_error)?,
                );
            }
        }
        transaction.commit().map_err(sqlite_error)?;
        Ok(result)
    }
}

#[allow(dead_code)] // The recovery receiver integrates this API separately.
pub(super) struct SuccessorQuery<'a> {
    pub(super) cluster_id: &'a str,
    pub(super) identity: &'a RepositoryNodeIdentity,
    pub(super) epoch: u64,
    pub(super) stream: &'a str,
    pub(super) first_sequence: u64,
    pub(super) end_sequence_exclusive: u64,
    pub(super) previous_hash: [u8; 32],
    pub(super) max_segments: usize,
    pub(super) max_wire_bytes: usize,
}

impl HistoryStorage {
    /// Read a contiguous signed successor page from one SQLite snapshot. The caller supplies the
    /// trusted predecessor hash and the durable source tail's exclusive sequence. On success the
    /// returned sequence and hash are the exact continuation; equality with the exclusive tail
    /// means the caller must compare that hash with the independently held durable source tail.
    /// An unindexed legacy row in this stream, a gap, overlap, fork, malformed wire or missing
    /// row fails the entire page. This method never repairs, acknowledges or changes the journal.
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // The recovery receiver integrates this API separately.
    pub(crate) fn source_delivery_journal_successors(
        &self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        epoch: u64,
        stream: &str,
        first_sequence: u64,
        end_sequence_exclusive: u64,
        previous_hash: [u8; 32],
        max_segments: usize,
        max_wire_bytes: usize,
    ) -> Result<(Vec<SourceDeliveryJournalRow>, u64, [u8; 32])> {
        let query = SuccessorQuery {
            cluster_id,
            identity,
            epoch,
            stream,
            first_sequence,
            end_sequence_exclusive,
            previous_hash,
            max_segments,
            max_wire_bytes,
        };
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("source delivery journal requires SQLite".into()))?;
        successors_in_snapshot(connection, &query, || {})
    }
}

// The callback is a test seam for a concurrent writer between metadata preflight and blob reads.
#[allow(dead_code)] // The recovery receiver integrates this API separately.
pub(super) fn successors_in_snapshot(
    connection: &mut Connection,
    query: &SuccessorQuery<'_>,
    after_preflight: impl FnOnce(),
) -> Result<(Vec<SourceDeliveryJournalRow>, u64, [u8; 32])> {
    use sha2::{Digest as _, Sha256};
    if query.cluster_id.is_empty()
        || query.cluster_id.len() > MAX_ID_BYTES
        || query.stream.len() > MAX_ID_BYTES
        || query.identity.node_id().as_str().len() > MAX_ID_BYTES
        || query.first_sequence == 0
        || query.first_sequence > query.end_sequence_exclusive
        || query.end_sequence_exclusive > (i64::MAX as u64) + 1
        || query.max_segments == 0
        || query.max_segments > SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS
        || query.max_wire_bytes == 0
        || query.max_wire_bytes > SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES
    {
        return Err(HistoryStorageError(
            "journal successor lookup invalid bounds".into(),
        ));
    }
    crate::history_sync::Cursor::new(
        query.identity.node_id().as_str(),
        query.epoch,
        query.stream,
        query.first_sequence,
    )
    .map_err(|error| HistoryStorageError(error.to_string()))?;
    let transaction = connection.transaction().map_err(sqlite_error)?;
    if query.first_sequence == query.end_sequence_exclusive {
        return Ok((Vec::new(), query.first_sequence, query.previous_hash));
    }
    let legacy: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM source_delivery_journal
          INDEXED BY source_delivery_journal_cursor_order
          WHERE stream=?1 AND source_node_id='')",
            [query.stream],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if legacy {
        return Err(HistoryStorageError(
            "journal successor lookup requires legacy order repair".into(),
        ));
    }
    // IDs and blobs are not materialized during this pass. Include one metadata lookahead to
    // detect a missing or overlapping segment even when the page stops at a count/byte limit.
    let metadata = {
        let mut statement = transaction
            .prepare(
                "SELECT rowid,first_sequence,length(wire),length(identity),
                    CASE WHEN length(id)<=1024 THEN length(CAST(id AS BLOB)) ELSE -1 END,
                    length(CAST(stream AS BLOB)),
                    typeof(wire),typeof(identity)
             FROM source_delivery_journal INDEXED BY source_delivery_journal_cursor_order
             WHERE stream=?1 AND source_node_id=?2 AND source_epoch=?3
               AND first_sequence>=?4 AND first_sequence<=?5
             ORDER BY first_sequence,created_at,id LIMIT ?6",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                params![
                    query.stream,
                    query.identity.node_id().as_str(),
                    durable_i64(query.epoch, "journal source epoch")?,
                    durable_i64(query.first_sequence, "journal first sequence")?,
                    durable_i64(query.end_sequence_exclusive - 1, "journal final sequence")?,
                    i64::try_from(query.max_segments + 1).unwrap()
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .map_err(sqlite_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)?
    };
    let mut selected = 0;
    let mut wire_bytes = 0_usize;
    let mut metadata_bytes = 0_usize;
    for (_, _, wire, identity, id, stream, wire_type, identity_type) in &metadata {
        let wire = usize::try_from(*wire)
            .map_err(|_| HistoryStorageError("invalid journal wire length".into()))?;
        let identity = usize::try_from(*identity)
            .map_err(|_| HistoryStorageError("invalid journal identity length".into()))?;
        let id = usize::try_from(*id)
            .map_err(|_| HistoryStorageError("invalid journal id length".into()))?;
        let stream = usize::try_from(*stream)
            .map_err(|_| HistoryStorageError("invalid journal stream length".into()))?;
        if wire_type != "blob"
            || identity_type != "blob"
            || id > MAX_ID_BYTES
            || wire > SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES
        {
            return Err(HistoryStorageError(
                "journal successor row exceeds bounds or has invalid type".into(),
            ));
        }
        if selected == query.max_segments || wire > query.max_wire_bytes.saturating_sub(wire_bytes)
        {
            break;
        }
        let extra = identity
            .checked_add(id)
            .and_then(|n| n.checked_add(stream))
            .ok_or_else(|| HistoryStorageError("journal successor metadata overflow".into()))?;
        if extra > MAX_METADATA_BYTES.saturating_sub(metadata_bytes) {
            return Err(HistoryStorageError(
                "journal successor metadata exceeds byte budget".into(),
            ));
        }
        metadata_bytes += extra;
        wire_bytes += wire;
        selected += 1;
    }
    if selected == 0 {
        return Err(HistoryStorageError(if metadata.is_empty() {
            "journal successor missing at expected sequence".into()
        } else {
            "journal successor exceeds page wire budget".into()
        }));
    }
    after_preflight();
    let mut result = Vec::with_capacity(selected);
    let mut next = query.first_sequence;
    let mut previous_hash = query.previous_hash;
    let mut statement = transaction
        .prepare(
            "SELECT id,stream,closed_at,identity,wire FROM source_delivery_journal WHERE rowid=?1",
        )
        .map_err(sqlite_error)?;
    for (rowid, first, _, _, _, _, _, _) in metadata.iter().take(selected) {
        if u64::try_from(*first).ok() != Some(next) {
            let message = if u64::try_from(*first).is_ok_and(|first| first > next) {
                "journal successor missing at expected sequence"
            } else {
                "journal successor duplicate or overlapping sequence"
            };
            return Err(HistoryStorageError(message.into()));
        }
        let row = statement
            .query_row([rowid], source_delivery_journal_row)
            .map_err(sqlite_error)?;
        if row.identity != *query.identity
            || row.stream != query.stream
            || hex::encode(Sha256::digest(&row.wire)) != row.id
        {
            return Err(HistoryStorageError(
                "journal successor identity or wire id mismatch".into(),
            ));
        }
        let signed = SignedSegment::from_wire(&row.wire)
            .map_err(|error| HistoryStorageError(error.to_string()))?;
        signed
            .verify_identity(query.identity)
            .map_err(|error| HistoryStorageError(error.to_string()))?;
        let canonical = signed.canonical();
        let first_cursor = canonical.first_cursor();
        if canonical.cluster_id() != query.cluster_id
            || first_cursor.source_node_id() != query.identity.node_id().as_str()
            || first_cursor.source_epoch() != query.epoch
            || first_cursor.stream() != query.stream
            || first_cursor.sequence() != next
            || canonical.previous_segment_hash() != Some(previous_hash)
            || canonical.closed_at_unix_seconds() != row.closed_at_unix_seconds
        {
            return Err(HistoryStorageError(
                "journal successor scope or predecessor mismatch".into(),
            ));
        }
        next = canonical
            .last_cursor()
            .sequence()
            .checked_add(1)
            .ok_or_else(|| HistoryStorageError("journal successor sequence overflow".into()))?;
        if next > query.end_sequence_exclusive {
            return Err(HistoryStorageError(
                "journal successor crosses requested source tail".into(),
            ));
        }
        previous_hash = signed
            .segment_hash()
            .map_err(|error| HistoryStorageError(error.to_string()))?;
        result.push(row);
    }
    if let Some((_, first, _, _, _, _, _, _)) = metadata.get(selected) {
        if u64::try_from(*first).is_ok_and(|first| first > next) {
            return Err(HistoryStorageError(
                "journal successor missing at expected sequence".into(),
            ));
        }
        if u64::try_from(*first).ok() != Some(next) || next == query.end_sequence_exclusive {
            return Err(HistoryStorageError(
                "journal successor missing, duplicate or overlapping sequence".into(),
            ));
        }
    } else if next != query.end_sequence_exclusive {
        return Err(HistoryStorageError(
            "journal successor missing before source tail".into(),
        ));
    }
    Ok((result, next, previous_hash))
}
