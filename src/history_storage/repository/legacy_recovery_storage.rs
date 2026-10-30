use super::super::*;
use crate::state::history_storage::{SourceDeliveryJournalRow, metrics};

const MAX_STATE: usize = 128 * 1024;
const PAGE_BYTES: usize = 256 * 1024;
const MIN_FREE_BYTES: u64 = 256 * 1024 * 1024;

fn schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS repository_legacy_recovery_state (
           singleton INTEGER PRIMARY KEY CHECK(singleton=1), payload BLOB NOT NULL);
         CREATE TABLE IF NOT EXISTS repository_legacy_recovery_wires (
           position INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE,
           stream TEXT NOT NULL, closed_at INTEGER NOT NULL,
           identity BLOB NOT NULL, wire BLOB NOT NULL);
         CREATE INDEX IF NOT EXISTS repository_legacy_recovery_stream_position
           ON repository_legacy_recovery_wires(stream, position);
         CREATE TABLE IF NOT EXISTS repository_legacy_recovery_suffix_wires (
           position INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE,
           stream TEXT NOT NULL, closed_at INTEGER NOT NULL,
           identity BLOB NOT NULL, wire BLOB NOT NULL);",
        )
        .map_err(sqlite_error)
}

fn checkpoint(connection: &Connection) -> Result<Option<Vec<u8>>> {
    let size: Option<i64> = connection
        .query_row(
            "SELECT length(payload) FROM repository_legacy_recovery_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)?;
    if size.is_some_and(|size| size < 0 || size as usize > MAX_STATE) {
        return Err(HistoryStorageError(
            "legacy recovery checkpoint exceeds bound".into(),
        ));
    }
    connection
        .query_row(
            "SELECT payload FROM repository_legacy_recovery_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)
}

impl HistoryStorage {
    /// Confirm exact page wires were staged as part of the verified source tombstone chain.
    pub(crate) fn legacy_recovery_staged_contains_rows(
        &self,
        rows: &[&SourceDeliveryJournalRow],
    ) -> Result<bool> {
        if rows.len() > 256 || rows.iter().map(|row| row.wire.len()).sum::<usize>() > 1024 * 1024 {
            return Err(HistoryStorageError(
                "legacy page proof exceeds bound".into(),
            ));
        }
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("legacy recovery requires SQLite".into()))?;
        schema(connection)?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        let mut query = transaction
            .prepare(
                "SELECT EXISTS(SELECT 1 FROM repository_legacy_recovery_wires
             WHERE id=?1 AND stream='tombstone' AND wire=?2)",
            )
            .map_err(sqlite_error)?;
        for row in rows {
            let found: bool = query
                .query_row(params![row.id, row.wire], |row| row.get(0))
                .map_err(sqlite_error)?;
            if !found {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Return one indexed local tombstone wire without advancing or acknowledging the journal.
    pub(crate) fn legacy_recovery_local_tombstone(
        &self,
        source: &str,
        epoch: u64,
        first_sequence: u64,
    ) -> Result<Option<SourceDeliveryJournalRow>> {
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("legacy recovery requires SQLite".into()))?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        let mut statement = transaction
            .prepare(
                "SELECT rowid, length(CAST(id AS BLOB)), length(wire),
                    length(identity), length(CAST(stream AS BLOB))
             FROM source_delivery_journal WHERE source_node_id=?1 AND source_epoch=?2
               AND stream='tombstone' AND first_sequence=?3 ORDER BY id LIMIT 2",
            )
            .map_err(sqlite_error)?;
        let mut rows = statement
            .query(params![
                source,
                durable_i64(epoch, "legacy epoch")?,
                durable_i64(first_sequence, "legacy sequence")?
            ])
            .map_err(sqlite_error)?;
        let Some(row) = rows.next().map_err(sqlite_error)? else {
            return Ok(None);
        };
        let rowid: i64 = row.get(0).map_err(sqlite_error)?;
        let id_len: usize = row.get(1).map_err(sqlite_error)?;
        let wire_len: usize = row.get(2).map_err(sqlite_error)?;
        let identity_len: usize = row.get(3).map_err(sqlite_error)?;
        let stream_len: usize = row.get(4).map_err(sqlite_error)?;
        if id_len != 64 || wire_len > PAGE_BYTES || identity_len > 4096 || stream_len > 256 {
            return Err(HistoryStorageError(
                "legacy local tombstone exceeds bound".into(),
            ));
        }
        if rows.next().map_err(sqlite_error)?.is_some() {
            return Err(HistoryStorageError(
                "duplicate local tombstone sequence".into(),
            ));
        }
        drop(rows);
        drop(statement);
        let row = transaction
            .query_row(
                "SELECT id,stream,closed_at,identity,wire FROM source_delivery_journal
                 WHERE rowid=?1",
                [rowid],
                staged_row,
            )
            .map_err(sqlite_error)?;
        Ok(Some(row))
    }

    pub(crate) fn legacy_recovery_checkpoint(&self) -> Result<Option<Vec<u8>>> {
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("legacy recovery requires SQLite".into()))?;
        schema(connection)?;
        checkpoint(connection)
    }

    /// Stage, receive rows, progress, final page ACK and control checkpoint share one transaction.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn commit_repository_legacy_recovery(
        &self,
        mutation: RepositoryReplicaMutation,
        control: &[u8],
        expected: Option<&[u8]>,
        next: &[u8],
        staged: &[SourceDeliveryJournalRow],
        suffix: &[SourceDeliveryJournalRow],
        ack_ids: &[String],
        finish: bool,
    ) -> Result<RepositoryCommitOutcome> {
        if next.len() > MAX_STATE
            || staged.len() > 64
            || suffix.len() > 64
            || ack_ids.len() > 256
            || staged
                .iter()
                .chain(suffix)
                .map(|row| row.wire.len())
                .sum::<usize>()
                > PAGE_BYTES
            || staged
                .iter()
                .chain(suffix)
                .any(|row| row.id.len() != 64 || row.stream.len() > 256)
        {
            return Err(HistoryStorageError(
                "legacy recovery transaction exceeds bound".into(),
            ));
        }
        if !staged.is_empty() || !suffix.is_empty() {
            let available = self.available_bytes().map_err(|error| {
                HistoryStorageError(format!("legacy recovery disk space: {error}"))
            })?;
            let page_bytes = staged
                .iter()
                .chain(suffix)
                .map(|row| row.wire.len() as u64)
                .sum::<u64>();
            if available < MIN_FREE_BYTES.saturating_add(page_bytes.saturating_mul(2)) {
                return Err(HistoryStorageError(
                    "legacy recovery needs additional free disk space".into(),
                ));
            }
        }
        if finish && ack_ids.is_empty() {
            let old: serde_json::Value = serde_json::from_slice(expected.ok_or_else(|| {
                HistoryStorageError("legacy recovery has no previous completed page".into())
            })?)
            .map_err(|error| HistoryStorageError(error.to_string()))?;
            let new: serde_json::Value = serde_json::from_slice(next)
                .map_err(|error| HistoryStorageError(error.to_string()))?;
            if old["phase"] != "done" || old["page_key"] == new["page_key"] {
                return Err(HistoryStorageError(
                    "legacy recovery cannot reset an active page".into(),
                ));
            }
        }
        self.commit_repository_replica_mutation_with(mutation, control, |transaction| {
            schema(transaction)?;
            if checkpoint(transaction)?.as_deref() != expected {
                return Err(HistoryStorageError(
                    "legacy recovery checkpoint changed".into(),
                ));
            }
            // The prior page has committed its receipt and cursor. Its signed segments and
            // records are in the repository; reclaim only this staging table for the next page.
            if finish && ack_ids.is_empty() {
                transaction
                    .execute("DELETE FROM repository_legacy_recovery_wires", [])
                    .map_err(sqlite_error)?;
                transaction
                    .execute("DELETE FROM repository_legacy_recovery_suffix_wires", [])
                    .map_err(sqlite_error)?;
            }
            for row in suffix {
                let identity = serde_json::to_vec(&row.identity)
                    .map_err(|error| HistoryStorageError(error.to_string()))?;
                if identity.len() > 4096 {
                    return Err(HistoryStorageError(
                        "legacy recovery identity exceeds bound".into(),
                    ));
                }
                transaction
                    .execute(
                        "INSERT INTO repository_legacy_recovery_suffix_wires
                     (id,stream,closed_at,identity,wire) VALUES (?1,?2,?3,?4,?5)",
                        params![
                            row.id,
                            row.stream,
                            durable_i64(row.closed_at_unix_seconds, "legacy suffix timestamp")?,
                            identity,
                            row.wire
                        ],
                    )
                    .map_err(sqlite_error)?;
            }
            for row in staged {
                let identity = serde_json::to_vec(&row.identity)
                    .map_err(|error| HistoryStorageError(error.to_string()))?;
                if identity.len() > 4096 {
                    return Err(HistoryStorageError(
                        "legacy recovery identity exceeds bound".into(),
                    ));
                }
                transaction
                    .execute(
                        "INSERT INTO repository_legacy_recovery_wires
                     (id,stream,closed_at,identity,wire) VALUES (?1,?2,?3,?4,?5)",
                        params![
                            row.id,
                            row.stream,
                            durable_i64(row.closed_at_unix_seconds, "legacy staged timestamp")?,
                            identity,
                            row.wire
                        ],
                    )
                    .map_err(sqlite_error)?;
            }
            if !ack_ids.is_empty() {
                acknowledge(transaction, ack_ids, &mutation_ack_time(control)?)?;
            }
            transaction
                .execute(
                    "INSERT INTO repository_legacy_recovery_state(singleton,payload) VALUES(1,?1)
                 ON CONFLICT(singleton) DO UPDATE SET payload=excluded.payload",
                    [next],
                )
                .map_err(sqlite_error)?;
            Ok(())
        })
    }

    #[cfg(test)]
    pub(crate) fn legacy_recovery_staged_after(
        &self,
        after: u64,
    ) -> Result<Vec<(u64, SourceDeliveryJournalRow)>> {
        self.legacy_recovery_staged_after_filtered(after, None)
    }

    pub(crate) fn legacy_recovery_staged_after_filtered(
        &self,
        after: u64,
        tombstones: Option<bool>,
    ) -> Result<Vec<(u64, SourceDeliveryJournalRow)>> {
        self.legacy_recovery_staged_page(after, tombstones, false)
    }

    pub(crate) fn legacy_recovery_suffix_after(
        &self,
        after: u64,
    ) -> Result<Vec<(u64, SourceDeliveryJournalRow)>> {
        self.legacy_recovery_staged_page(after, None, true)
    }

    fn legacy_recovery_staged_page(
        &self,
        after: u64,
        tombstones: Option<bool>,
        suffix: bool,
    ) -> Result<Vec<(u64, SourceDeliveryJournalRow)>> {
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("legacy recovery requires SQLite".into()))?;
        schema(connection)?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        // Inspect lengths before fetching any blob, including identity JSON.
        let filter = match tombstones {
            Some(true) => " AND stream='tombstone'",
            Some(false) => " AND stream!='tombstone'",
            None => "",
        };
        let table = if suffix {
            "repository_legacy_recovery_suffix_wires"
        } else {
            "repository_legacy_recovery_wires"
        };
        let sql = format!(
            "SELECT position,length(wire),length(identity),length(CAST(stream AS BLOB)) \
             FROM {table} WHERE position>?1{filter}
             ORDER BY position LIMIT 64"
        );
        let mut lengths = transaction.prepare(&sql).map_err(sqlite_error)?;
        let lengths = lengths
            .query_map([durable_i64(after, "legacy apply position")?], |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, usize>(2)?,
                    row.get::<_, usize>(3)?,
                ))
            })
            .map_err(sqlite_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)?;
        let mut budget = 0;
        let mut result = Vec::new();
        for (position, wire_len, identity_len, stream_len) in lengths {
            if identity_len > 4096 || stream_len > 256 || wire_len > PAGE_BYTES {
                return Err(HistoryStorageError(
                    "invalid legacy staged row length".into(),
                ));
            }
            if wire_len > PAGE_BYTES - budget {
                break;
            }
            budget += wire_len;
            let row_sql =
                format!("SELECT id,stream,closed_at,identity,wire FROM {table} WHERE position=?1");
            let row = transaction
                .query_row(
                    &row_sql,
                    [durable_i64(position, "legacy position")?],
                    staged_row,
                )
                .map_err(sqlite_error)?;
            result.push((position, row));
        }
        Ok(result)
    }
}

fn mutation_ack_time(control: &[u8]) -> Result<u64> {
    let value: serde_json::Value =
        serde_json::from_slice(control).map_err(|error| HistoryStorageError(error.to_string()))?;
    value["last_verified_unix_seconds"]
        .as_u64()
        .ok_or_else(|| HistoryStorageError("legacy ACK lacks verified receiver time".into()))
}

fn acknowledge(transaction: &rusqlite::Transaction<'_>, ids: &[String], now: &u64) -> Result<()> {
    use crate::history_sync::SignedSegment;
    use sha2::{Digest as _, Sha256};
    let mut seen = std::collections::BTreeSet::new();
    let mut wire_bytes = 0_u64;
    let mut canonical_bytes = 0_u64;
    let mut scopes = std::collections::BTreeMap::<(String, String), (u64, u64)>::new();
    let mut replay_stream = None;
    // Exact journal lookup was preflighted by the receiver. Recheck every row in this transaction.
    for id in ids {
        if !seen.insert(id) {
            return Err(HistoryStorageError("duplicate legacy ACK ID".into()));
        }
        let len: usize = transaction
            .query_row(
                "SELECT length(wire) FROM source_delivery_journal WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        if len as u64 > 1024 * 1024 - wire_bytes {
            return Err(HistoryStorageError("legacy ACK wire bound exceeded".into()));
        }
        wire_bytes += len as u64;
        let (source, stream, wire): (String, String, Vec<u8>) = transaction
            .query_row(
                "SELECT source_node_id,stream,wire FROM source_delivery_journal WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(sqlite_error)?;
        if hex::encode(Sha256::digest(&wire)) != *id {
            return Err(HistoryStorageError(
                "legacy ACK journal wire changed".into(),
            ));
        }
        replay_stream = Some(stream.clone());
        let canonical = SignedSegment::from_wire(&wire)
            .and_then(|signed| signed.canonical().canonical_bytes())
            .map_err(|error| HistoryStorageError(error.to_string()))?
            .len() as u64;
        canonical_bytes += canonical;
        let entry = scopes.entry((source, stream)).or_default();
        entry.0 += 1;
        entry.1 += canonical;
        if transaction
            .execute("DELETE FROM source_delivery_journal WHERE id=?1", [id])
            .map_err(sqlite_error)?
            != 1
        {
            return Err(HistoryStorageError("legacy ACK row changed".into()));
        }
    }
    for ((source, stream), (count, bytes)) in scopes {
        if transaction
            .execute(
                "UPDATE source_delivery_journal_stream_state
             SET pending_segments=pending_segments-?1 WHERE stream=?2 AND pending_segments>=?1",
                params![count, stream],
            )
            .map_err(sqlite_error)?
            != 1
        {
            return Err(HistoryStorageError(
                "legacy ACK stream accounting mismatch".into(),
            ));
        }
        metrics::increment_acknowledged_scope(
            transaction,
            &source,
            &stream,
            "legacy_original_recovery",
            count,
            bytes,
            Some(*now),
        )?;
    }
    if transaction
        .execute(
            "UPDATE source_delivery_journal_state SET pending_segments=pending_segments-?1,
         pending_bytes=pending_bytes-?2,last_acknowledged_at=?3,
         last_delivery_path='legacy_original_recovery',replay_stream_cursor=?4
         WHERE singleton=1 AND pending_segments>=?1 AND pending_bytes>=?2",
            params![
                ids.len() as u64,
                wire_bytes,
                durable_i64(*now, "legacy ACK timestamp")?,
                replay_stream
            ],
        )
        .map_err(sqlite_error)?
        != 1
    {
        return Err(HistoryStorageError(
            "legacy ACK journal accounting mismatch".into(),
        ));
    }
    transaction
        .execute(
            "UPDATE source_delivery_journal_state SET capacity_suspended=CASE
         WHEN pending_segments*100 < 20000*60 AND pending_bytes*100 < 134217728*60
         THEN 0 ELSE capacity_suspended END WHERE singleton=1",
            [],
        )
        .map_err(sqlite_error)?;
    metrics::increment_acknowledged(transaction, ids.len() as u64, canonical_bytes, Some(*now))
}

fn staged_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceDeliveryJournalRow> {
    let bytes: Vec<u8> = row.get(3)?;
    let identity = serde_json::from_slice(&bytes).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Blob, Box::new(error))
    })?;
    Ok(SourceDeliveryJournalRow {
        id: row.get(0)?,
        stream: row.get(1)?,
        closed_at_unix_seconds: row.get(2)?,
        identity,
        wire: row.get(4)?,
    })
}
