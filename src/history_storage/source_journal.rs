use serde::{Deserialize, Serialize};

use crate::history_sync::SignedSegment;
use crate::state::history_repository::identity::RepositoryNodeIdentity;

use super::*;

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

    pub(crate) fn source_delivery_journal(&self) -> Result<Vec<SourceDeliveryJournalRow>> {
        self.source_delivery_journal_page(usize::MAX)
    }

    pub(crate) fn source_delivery_journal_page(
        &self,
        limit: usize,
    ) -> Result<Vec<SourceDeliveryJournalRow>> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(Vec::new());
        };
        let mut statement = connection
            .prepare(
                "SELECT id, stream, closed_at, identity, wire
                 FROM source_delivery_journal
                 ORDER BY (stream = 'tombstone') DESC, created_at, id
                 LIMIT ?1",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
                let identity_bytes: Vec<u8> = row.get(3)?;
                let identity = serde_json::from_slice(&identity_bytes).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Blob,
                        Box::new(error),
                    )
                })?;
                Ok(SourceDeliveryJournalRow {
                    id: row.get(0)?,
                    stream: row.get(1)?,
                    closed_at_unix_seconds: u64::try_from(row.get::<_, i64>(2)?)
                        .unwrap_or(u64::MAX),
                    identity,
                    wire: row.get(4)?,
                })
            })
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

fn insert_journal_rows(
    transaction: &rusqlite::Transaction<'_>,
    rows: &[SourceDeliveryJournalRow],
) -> Result<()> {
    for row in rows {
        let identity = serde_json::to_vec(&row.identity)
            .map_err(|error| HistoryStorageError(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO source_delivery_journal
                     (id, stream, closed_at, identity, wire, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                     stream = excluded.stream,
                     closed_at = excluded.closed_at,
                     identity = excluded.identity,
                     wire = excluded.wire",
                params![
                    row.id,
                    row.stream,
                    durable_i64(row.closed_at_unix_seconds, "journal closed_at")?,
                    identity,
                    row.wire,
                    durable_i64(row.closed_at_unix_seconds, "journal created_at")?,
                ],
            )
            .map_err(sqlite_error)?;
    }
    Ok(())
}
