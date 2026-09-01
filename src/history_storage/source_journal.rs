use serde::{Deserialize, Serialize};

use crate::history_sync::SignedSegment;
use crate::state::history_repository::identity::RepositoryNodeIdentity;

use super::*;

const SOURCE_DELIVERY_JOURNAL_REPAIR_PAGE_SIZE: i64 = 256;

pub(super) fn ensure_source_delivery_journal_columns(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction().map_err(sqlite_error)?;
    let columns = {
        let mut statement = transaction
            .prepare("PRAGMA table_info(source_delivery_journal)")
            .map_err(sqlite_error)?;
        statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(sqlite_error)?
            .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()
            .map_err(sqlite_error)?
    };
    let journal_order_columns_missing = ["source_node_id", "source_epoch", "first_sequence"]
        .iter()
        .any(|name| !columns.contains(*name));
    for (name, definition) in [
        ("source_node_id", "TEXT NOT NULL DEFAULT ''"),
        ("source_epoch", "INTEGER NOT NULL DEFAULT 0"),
        ("first_sequence", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !columns.contains(name) {
            transaction
                .execute(
                    &format!("ALTER TABLE source_delivery_journal ADD COLUMN {name} {definition}"),
                    [],
                )
                .map_err(sqlite_error)?;
        }
    }
    transaction
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS source_delivery_journal_cursor_order
               ON source_delivery_journal
                  (stream, source_node_id, source_epoch, first_sequence, created_at, id);",
        )
        .map_err(sqlite_error)?;
    transaction
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS source_delivery_journal_delivery_order
               ON source_delivery_journal
                  ((stream = 'tombstone') DESC, source_node_id, source_epoch, stream,
                   first_sequence, created_at, id);
             CREATE TABLE IF NOT EXISTS source_delivery_journal_state (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 pending_segments INTEGER NOT NULL,
                 pending_bytes INTEGER NOT NULL,
                 epoch_high_water INTEGER NOT NULL,
                 last_acknowledged_at INTEGER,
                 last_delivery_path TEXT,
                 order_repair_cursor_id TEXT,
                 order_repair_completed INTEGER NOT NULL DEFAULT 0
             );",
        )
        .map_err(sqlite_error)?;
    let state_columns = {
        let mut state_statement = transaction
            .prepare("PRAGMA table_info(source_delivery_journal_state)")
            .map_err(sqlite_error)?;
        state_statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(sqlite_error)?
            .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()
            .map_err(sqlite_error)?
    };
    let state_order_columns_missing = ["order_repair_cursor_id", "order_repair_completed"]
        .iter()
        .any(|name| !state_columns.contains(*name));
    for (name, definition) in [
        ("order_repair_cursor_id", "TEXT"),
        ("order_repair_completed", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !state_columns.contains(name) {
            transaction
                .execute(
                    &format!(
                        "ALTER TABLE source_delivery_journal_state ADD COLUMN {name} {definition}"
                    ),
                    [],
                )
                .map_err(sqlite_error)?;
        }
    }
    let state_row_exists = transaction
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM source_delivery_journal_state WHERE singleton = 1
             )",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sqlite_error)?
        != 0;
    if !state_row_exists {
        transaction
            .execute(
                "INSERT INTO source_delivery_journal_state
                     (singleton, pending_segments, pending_bytes, epoch_high_water,
                      order_repair_cursor_id, order_repair_completed)
                 SELECT 1, COUNT(*), COALESCE(SUM(length(wire)), 0), COALESCE(MAX(source_epoch), 0),
                        NULL,
                        CASE WHEN COALESCE(
                            SUM(CASE WHEN source_node_id = '' THEN 1 ELSE 0 END), 0
                        ) > 0 THEN 0 ELSE 1 END
                 FROM source_delivery_journal",
                [],
            )
            .map_err(sqlite_error)?;
    }
    // Only classify legacy rows while introducing the state marker. Once the marker is present,
    // its durable value is authoritative and startup remains a constant-cost schema check. A
    // newly inserted state row already receives the classification from the aggregate above.
    if state_order_columns_missing || journal_order_columns_missing {
        let legacy_rows_exist = if journal_order_columns_missing {
            transaction
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM source_delivery_journal)",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(sqlite_error)?
        } else {
            transaction
                .query_row(
                    "SELECT EXISTS (
                         SELECT 1 FROM source_delivery_journal WHERE source_node_id = ''
                     )",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(sqlite_error)?
        } != 0;
        transaction
            .execute(
                "UPDATE source_delivery_journal_state
                 SET order_repair_cursor_id = CASE
                         WHEN ?1 != 0 THEN NULL ELSE order_repair_cursor_id END,
                     order_repair_completed = CASE WHEN ?1 != 0 THEN 0 ELSE 1 END
                 WHERE singleton = 1",
                [if legacy_rows_exist { 1_i64 } else { 0_i64 }],
            )
            .map_err(sqlite_error)?;
    }
    transaction.commit().map_err(sqlite_error)
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
    pub(crate) last_acknowledged_at: Option<u64>,
    pub(crate) last_delivery_path: Option<String>,
    pub(crate) order_repairing: bool,
}

#[derive(Debug)]
pub(crate) enum SourceDeliveryJournalPage {
    Ready(Vec<SourceDeliveryJournalRow>),
    Repairing,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct SourceDeliveryJournalRepairProgress {
    pub(crate) processed: usize,
    pub(crate) completed: bool,
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
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(Vec::new());
        };
        let order_repair_completed = connection
            .query_row(
                "SELECT order_repair_completed
                 FROM source_delivery_journal_state
                 WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_error)?;
        if order_repair_completed == 0 {
            return Err(HistoryStorageError(
                "source delivery journal order repair is still in progress".to_owned(),
            ));
        }
        let mut statement = connection
            .prepare(
                "SELECT id, stream, closed_at, identity, wire
                 FROM source_delivery_journal
                 ORDER BY (stream = 'tombstone') DESC, source_node_id, source_epoch,
                          stream, first_sequence, created_at, id",
            )
            .map_err(sqlite_error)?;
        statement
            .query_map([], source_delivery_journal_row)
            .map_err(sqlite_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)
    }

    pub(crate) fn source_delivery_journal_summary(&self) -> Result<SourceDeliveryJournalSummary> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(SourceDeliveryJournalSummary {
                pending_segments: 0,
                pending_bytes: 0,
                oldest: None,
                last_acknowledged_at: None,
                last_delivery_path: None,
                order_repairing: false,
            });
        };
        let state = connection
            .query_row(
                "SELECT pending_segments, pending_bytes, last_acknowledged_at,
                        last_delivery_path, order_repair_completed
                 FROM source_delivery_journal_state
                 WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .map_err(sqlite_error)?;
        let order_repairing = state.4 == 0;
        let oldest = if order_repairing {
            None
        } else {
            connection
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
                .map_err(sqlite_error)?
        };
        Ok(SourceDeliveryJournalSummary {
            pending_segments: usize::try_from(state.0).unwrap_or(usize::MAX),
            pending_bytes: u64::try_from(state.1).unwrap_or(u64::MAX),
            oldest,
            last_acknowledged_at: state.2.and_then(|value| u64::try_from(value).ok()),
            last_delivery_path: state.3,
            order_repairing,
        })
    }

    pub(crate) fn source_delivery_journal_page(
        &self,
        limit: usize,
    ) -> Result<SourceDeliveryJournalPage> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(SourceDeliveryJournalPage::Ready(Vec::new()));
        };
        let order_repair_completed = connection
            .query_row(
                "SELECT order_repair_completed
                 FROM source_delivery_journal_state
                 WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_error)?;
        if order_repair_completed == 0 {
            return Ok(SourceDeliveryJournalPage::Repairing);
        }
        let limit = limit.min(SOURCE_DELIVERY_JOURNAL_REPAIR_PAGE_SIZE as usize);
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
        Ok(SourceDeliveryJournalPage::Ready(
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sqlite_error)?,
        ))
    }

    pub(crate) fn repair_source_delivery_journal_order_page(
        &self,
    ) -> Result<SourceDeliveryJournalRepairProgress> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(SourceDeliveryJournalRepairProgress {
                processed: 0,
                completed: true,
            });
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        let (cursor, order_repair_completed) = transaction
            .query_row(
                "SELECT order_repair_cursor_id, order_repair_completed
                 FROM source_delivery_journal_state
                 WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(sqlite_error)?;
        if order_repair_completed != 0 {
            transaction.commit().map_err(sqlite_error)?;
            return Ok(SourceDeliveryJournalRepairProgress {
                processed: 0,
                completed: true,
            });
        }
        let rows = if let Some(cursor) = cursor.as_deref() {
            let mut statement = transaction
                .prepare(
                    "SELECT id, source_node_id
                     FROM source_delivery_journal
                     WHERE id > ?1
                     ORDER BY id
                     LIMIT ?2",
                )
                .map_err(sqlite_error)?;
            statement
                .query_map(
                    params![cursor, SOURCE_DELIVERY_JOURNAL_REPAIR_PAGE_SIZE],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .map_err(sqlite_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sqlite_error)?
        } else {
            let mut statement = transaction
                .prepare(
                    "SELECT id, source_node_id
                     FROM source_delivery_journal
                     ORDER BY id
                     LIMIT ?1",
                )
                .map_err(sqlite_error)?;
            statement
                .query_map([SOURCE_DELIVERY_JOURNAL_REPAIR_PAGE_SIZE], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(sqlite_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sqlite_error)?
        };
        for (id, source_node_id) in &rows {
            if !source_node_id.is_empty() {
                continue;
            }
            let wire = transaction
                .query_row(
                    "SELECT wire FROM source_delivery_journal WHERE id = ?1",
                    [id],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .map_err(sqlite_error)?;
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
            transaction
                .execute(
                    "UPDATE source_delivery_journal_state
                     SET epoch_high_water = MAX(epoch_high_water, ?1)
                     WHERE singleton = 1",
                    [durable_i64(cursor.source_epoch(), "journal source epoch")?],
                )
                .map_err(sqlite_error)?;
        }
        let completed = rows.len() < SOURCE_DELIVERY_JOURNAL_REPAIR_PAGE_SIZE as usize;
        let next_cursor = rows.last().map(|(id, _)| id.as_str());
        transaction
            .execute(
                "UPDATE source_delivery_journal_state
                 SET order_repair_cursor_id = COALESCE(?1, order_repair_cursor_id),
                     order_repair_completed = ?2
                 WHERE singleton = 1",
                params![next_cursor, if completed { 1_i64 } else { 0_i64 }],
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)?;
        Ok(SourceDeliveryJournalRepairProgress {
            processed: rows.len(),
            completed,
        })
    }

    pub(crate) fn source_delivery_journal_max_epoch(&self) -> Result<Option<u64>> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(None);
        };
        connection
            .query_row(
                "SELECT epoch_high_water
                 FROM source_delivery_journal_state
                 WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(sqlite_error)?
            .map(|value| {
                u64::try_from(value)
                    .map_err(|_| HistoryStorageError("negative journal epoch".to_owned()))
            })
            .transpose()
    }

    pub(crate) fn acknowledge_source_delivery_journal(
        &self,
        ids: &[String],
        acknowledged_at_unix_seconds: Option<u64>,
        delivery_path: Option<&str>,
    ) -> Result<()> {
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
        let mut deleted_any = false;
        for id in ids {
            let Some(wire_len) = transaction
                .query_row(
                    "SELECT length(wire) FROM source_delivery_journal WHERE id = ?1",
                    [id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(sqlite_error)?
            else {
                continue;
            };
            let deleted = transaction
                .execute("DELETE FROM source_delivery_journal WHERE id = ?1", [id])
                .map_err(sqlite_error)?;
            if deleted == 1 {
                deleted_any = true;
                transaction
                    .execute(
                        "UPDATE source_delivery_journal_state
                         SET pending_segments = pending_segments - 1,
                             pending_bytes = pending_bytes - ?1
                         WHERE singleton = 1",
                        [wire_len],
                    )
                    .map_err(sqlite_error)?;
            }
        }
        if deleted_any && let Some(acknowledged_at) = acknowledged_at_unix_seconds {
            transaction
                .execute(
                    "UPDATE source_delivery_journal_state
                     SET last_acknowledged_at = ?1,
                         last_delivery_path = COALESCE(?2, last_delivery_path)
                     WHERE singleton = 1",
                    params![
                        durable_i64(acknowledged_at, "journal acknowledgement time")?,
                        delivery_path,
                    ],
                )
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
        let previous_wire_len = transaction
            .query_row(
                "SELECT length(wire) FROM source_delivery_journal WHERE id = ?1",
                [&row.id],
                |query_row| query_row.get::<_, i64>(0),
            )
            .optional()
            .map_err(sqlite_error)?;
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
        let wire_len = i64::try_from(row.wire.len()).map_err(|_| {
            HistoryStorageError("journal wire length exceeds SQLite integer".to_owned())
        })?;
        match previous_wire_len {
            Some(previous_wire_len) => {
                let delta = wire_len.checked_sub(previous_wire_len).ok_or_else(|| {
                    HistoryStorageError("journal wire length delta overflow".to_owned())
                })?;
                transaction
                    .execute(
                        "UPDATE source_delivery_journal_state
                         SET pending_bytes = pending_bytes + ?1,
                             epoch_high_water = MAX(epoch_high_water, ?2)
                         WHERE singleton = 1",
                        params![
                            delta,
                            durable_i64(cursor.source_epoch(), "journal source epoch")?
                        ],
                    )
                    .map_err(sqlite_error)?;
            }
            None => {
                transaction
                    .execute(
                        "UPDATE source_delivery_journal_state
                         SET pending_segments = pending_segments + 1,
                             pending_bytes = pending_bytes + ?1,
                             epoch_high_water = MAX(epoch_high_water, ?2)
                         WHERE singleton = 1",
                        params![
                            wire_len,
                            durable_i64(cursor.source_epoch(), "journal source epoch")?
                        ],
                    )
                    .map_err(sqlite_error)?;
            }
        }
    }
    Ok(())
}
