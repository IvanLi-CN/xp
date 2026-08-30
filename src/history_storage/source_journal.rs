use serde::{Deserialize, Serialize};

use crate::history_sync::SignedSegment;
use crate::state::history_repository::identity::RepositoryNodeIdentity;

use super::*;

const SOURCE_DELIVERY_JOURNAL_REPAIR_PAGE_SIZE: i64 = 256;

pub(super) fn ensure_source_delivery_journal_columns(connection: &Connection) -> Result<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(source_delivery_journal)")
        .map_err(sqlite_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite_error)?
        .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()
        .map_err(sqlite_error)?;
    for (name, definition) in [
        ("source_node_id", "TEXT NOT NULL DEFAULT ''"),
        ("source_epoch", "INTEGER NOT NULL DEFAULT 0"),
        ("first_sequence", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !columns.contains(name) {
            connection
                .execute(
                    &format!("ALTER TABLE source_delivery_journal ADD COLUMN {name} {definition}"),
                    [],
                )
                .map_err(sqlite_error)?;
        }
    }
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS source_delivery_journal_cursor_order
               ON source_delivery_journal
                  (stream, source_node_id, source_epoch, first_sequence, created_at, id);",
        )
        .map_err(sqlite_error)
}

/// A signed source segment that has not yet been acknowledged by every required repository.
/// The payload is kept in SQLite so a control snapshot cannot grow with a delivery outage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SourceDeliveryJournalRow {
    pub(crate) id: String,
    pub(crate) stream: String,
    pub(crate) closed_at_unix_seconds: u64,
    pub(crate) identity: RepositoryNodeIdentity,
    pub(crate) wire: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct SourceDeliveryJournalSummary {
    pub(crate) pending_segments: usize,
    pub(crate) pending_bytes: u64,
    pub(crate) oldest: Option<SourceDeliveryJournalRow>,
}

impl HistoryStorage {
    pub(crate) fn append_source_delivery_journal_and_control(
        &self,
        rows: &[SourceDeliveryJournalRow],
        control_payload: &[u8],
    ) -> Result<()> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Err(HistoryStorageError(
                "source delivery journal requires SQLite".to_owned(),
            ));
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        insert_journal_rows(&transaction, rows)?;
        write_snapshot(&transaction, REPOSITORY_REPLICA_KEY, control_payload)?;
        transaction.commit().map_err(sqlite_error)?;
        maintain_sqlite(connection)
    }

    pub(crate) fn append_source_delivery_journal(
        &self,
        rows: &[SourceDeliveryJournalRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Err(HistoryStorageError(
                "source delivery journal requires SQLite".to_owned(),
            ));
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        insert_journal_rows(&transaction, rows)?;
        transaction.commit().map_err(sqlite_error)?;
        maintain_sqlite(connection)
    }

    #[cfg(test)]
    pub(crate) fn source_delivery_journal(&self) -> Result<Vec<SourceDeliveryJournalRow>> {
        self.source_delivery_journal_page(usize::MAX)
    }

    pub(crate) fn source_delivery_journal_summary(&self) -> Result<SourceDeliveryJournalSummary> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(SourceDeliveryJournalSummary {
                pending_segments: 0,
                pending_bytes: 0,
                oldest: None,
            });
        };
        repair_source_delivery_journal_order(connection)?;
        let (pending_segments, pending_bytes) = connection
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(length(wire)), 0)
                 FROM source_delivery_journal",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(sqlite_error)?;
        let oldest = connection
            .query_row(
                "SELECT id, stream, closed_at, identity, wire
                 FROM source_delivery_journal
                 ORDER BY (stream = 'tombstone') DESC, source_node_id, source_epoch,
                          stream, first_sequence, created_at, id
                 LIMIT 1",
                [],
                source_delivery_journal_row,
            )
            .optional()
            .map_err(sqlite_error)?;
        Ok(SourceDeliveryJournalSummary {
            pending_segments: usize::try_from(pending_segments).unwrap_or(usize::MAX),
            pending_bytes: u64::try_from(pending_bytes).unwrap_or(u64::MAX),
            oldest,
        })
    }

    pub(crate) fn source_delivery_journal_page(
        &self,
        limit: usize,
    ) -> Result<Vec<SourceDeliveryJournalRow>> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(Vec::new());
        };
        repair_source_delivery_journal_order(connection)?;
        let mut statement = connection
            .prepare(
                "SELECT id, stream, closed_at, identity, wire
                 FROM source_delivery_journal
                 ORDER BY (stream = 'tombstone') DESC, source_node_id, source_epoch,
                          stream, first_sequence, created_at, id
                 LIMIT ?1",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                [i64::try_from(limit).unwrap_or(i64::MAX)],
                source_delivery_journal_row,
            )
            .map_err(sqlite_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)
    }

    pub(crate) fn source_delivery_journal_max_epoch(&self) -> Result<Option<u64>> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(None);
        };
        let mut statement = connection
            .prepare("SELECT wire FROM source_delivery_journal")
            .map_err(sqlite_error)?;
        let mut rows = statement.query([]).map_err(sqlite_error)?;
        let mut max_epoch = None;
        while let Some(row) = rows.next().map_err(sqlite_error)? {
            let wire: Vec<u8> = row.get(0).map_err(sqlite_error)?;
            let segment = SignedSegment::from_wire(&wire).map_err(|error| {
                HistoryStorageError(format!("invalid source delivery journal wire: {error}"))
            })?;
            let epoch = segment.canonical().first_cursor().source_epoch();
            max_epoch = Some(max_epoch.map_or(epoch, |current: u64| current.max(epoch)));
        }
        Ok(max_epoch)
    }

    pub(crate) fn acknowledge_source_delivery_journal(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Err(HistoryStorageError(
                "source delivery journal requires SQLite".to_owned(),
            ));
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        for id in ids {
            transaction
                .execute("DELETE FROM source_delivery_journal WHERE id = ?1", [id])
                .map_err(sqlite_error)?;
        }
        transaction.commit().map_err(sqlite_error)?;
        maintain_sqlite(connection)
    }
}

fn source_delivery_journal_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SourceDeliveryJournalRow> {
    let identity_bytes: Vec<u8> = row.get(3)?;
    let identity = serde_json::from_slice(&identity_bytes).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Blob, Box::new(error))
    })?;
    Ok(SourceDeliveryJournalRow {
        id: row.get(0)?,
        stream: row.get(1)?,
        closed_at_unix_seconds: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(u64::MAX),
        identity,
        wire: row.get(4)?,
    })
}

fn insert_journal_rows(
    transaction: &rusqlite::Transaction<'_>,
    rows: &[SourceDeliveryJournalRow],
) -> Result<()> {
    for row in rows {
        let identity = serde_json::to_vec(&row.identity)
            .map_err(|error| HistoryStorageError(error.to_string()))?;
        let segment = SignedSegment::from_wire(&row.wire).map_err(|error| {
            HistoryStorageError(format!("invalid source delivery journal wire: {error}"))
        })?;
        let cursor = segment.canonical().first_cursor();
        transaction
            .execute(
                "INSERT INTO source_delivery_journal
                     (id, stream, closed_at, identity, wire, created_at,
                      source_node_id, source_epoch, first_sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                     stream = excluded.stream,
                     closed_at = excluded.closed_at,
                     identity = excluded.identity,
                     wire = excluded.wire,
                     source_node_id = excluded.source_node_id,
                     source_epoch = excluded.source_epoch,
                     first_sequence = excluded.first_sequence",
                params![
                    row.id,
                    row.stream,
                    durable_i64(row.closed_at_unix_seconds, "journal closed_at")?,
                    identity,
                    row.wire,
                    durable_i64(row.closed_at_unix_seconds, "journal created_at")?,
                    cursor.source_node_id(),
                    durable_i64(cursor.source_epoch(), "journal source epoch")?,
                    durable_i64(cursor.sequence(), "journal first sequence")?,
                ],
            )
            .map_err(sqlite_error)?;
    }
    Ok(())
}

fn repair_source_delivery_journal_order(connection: &mut rusqlite::Connection) -> Result<()> {
    loop {
        let rows = {
            let mut statement = connection
                .prepare(
                    "SELECT id, wire FROM source_delivery_journal
                     WHERE source_node_id = ''
                     ORDER BY id
                     LIMIT ?1",
                )
                .map_err(sqlite_error)?;
            statement
                .query_map([SOURCE_DELIVERY_JOURNAL_REPAIR_PAGE_SIZE], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(sqlite_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sqlite_error)?
        };
        if rows.is_empty() {
            return Ok(());
        }
        let transaction = connection.transaction().map_err(sqlite_error)?;
        for (id, wire) in rows {
            let segment = SignedSegment::from_wire(&wire).map_err(|error| {
                HistoryStorageError(format!("invalid source delivery journal wire: {error}"))
            })?;
            let cursor = segment.canonical().first_cursor();
            transaction
                .execute(
                    "UPDATE source_delivery_journal
                     SET source_node_id = ?1, source_epoch = ?2, first_sequence = ?3
                     WHERE id = ?4",
                    params![
                        cursor.source_node_id(),
                        durable_i64(cursor.source_epoch(), "journal source epoch")?,
                        durable_i64(cursor.sequence(), "journal first sequence")?,
                        id,
                    ],
                )
                .map_err(sqlite_error)?;
        }
        transaction.commit().map_err(sqlite_error)?;
    }
}
