use super::*;

const ROWS_PER_PAGE: usize = 128;
const BYTES_PER_PAGE: usize = 1024 * 1024;

impl HistoryStorage {
    /// Re-encode one bounded page in each table without changing the logical row or signed wire.
    /// Cursor and rewrites commit together. New writes already use the compact encoding.
    pub(crate) fn compact_repository_payload_page(&self) -> Result<usize> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(0);
        };
        let mut rewritten = 0;
        let mut saved_bytes = 0usize;
        for (table, cursor_key) in [
            (
                "repository_history_records",
                "repository_payload_records_cursor_v1",
            ),
            (
                "repository_history_segments",
                "repository_payload_segments_cursor_v1",
            ),
        ] {
            let transaction = connection.transaction().map_err(sqlite_error)?;
            let after = transaction
                .query_row(
                    "SELECT value FROM history_storage_meta WHERE key = ?1",
                    [cursor_key],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(sqlite_error)?
                .unwrap_or(0);
            if after < 0 {
                continue;
            }
            let mut statement = transaction
                .prepare(&format!(
                    "SELECT rowid, payload FROM {table} WHERE rowid > ?1 ORDER BY rowid LIMIT ?2"
                ))
                .map_err(sqlite_error)?;
            let mut rows = statement
                .query(params![after, ROWS_PER_PAGE as i64])
                .map_err(sqlite_error)?;
            let mut updates = Vec::new();
            let mut bytes = 0usize;
            let mut last = after;
            let mut complete = false;
            let mut seen = 0usize;
            loop {
                let Some(row) = rows.next().map_err(sqlite_error)? else {
                    complete = seen < ROWS_PER_PAGE;
                    break;
                };
                let payload: Vec<u8> = row.get(1).map_err(sqlite_error)?;
                if seen > 0 && bytes.saturating_add(payload.len()) > BYTES_PER_PAGE {
                    break;
                }
                seen += 1;
                last = row.get(0).map_err(sqlite_error)?;
                bytes = bytes.saturating_add(payload.len());
                // Revalidate an already compact row, but do not compress its frame a second time.
                let original = payload_codec::decode(payload.clone())?;
                let encoded = payload_codec::encode(&original)?;
                if encoded != payload {
                    saved_bytes += payload.len().saturating_sub(encoded.len());
                    updates.push((last, encoded));
                }
                if bytes >= BYTES_PER_PAGE {
                    break;
                }
            }
            drop(rows);
            drop(statement);
            for (rowid, payload) in &updates {
                transaction
                    .execute(
                        &format!("UPDATE {table} SET payload = ?1 WHERE rowid = ?2"),
                        params![payload, rowid],
                    )
                    .map_err(sqlite_error)?;
            }
            transaction
                .execute(
                    "INSERT INTO history_storage_meta(key,value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![cursor_key, if complete { -1 } else { last }],
                )
                .map_err(sqlite_error)?;
            transaction.commit().map_err(sqlite_error)?;
            rewritten += updates.len();
        }
        // Release up to 2 MiB of free pages without a full VACUUM, extra database,
        // or manual WAL checkpoint. A large legacy row may occupy a page on its own.
        connection
            .execute_batch("PRAGMA incremental_vacuum(512)")
            .map_err(sqlite_error)?;
        if rewritten > 0 {
            tracing::info!(
                rewritten,
                saved_bytes,
                "history repository payload encoding page committed"
            );
        }
        Ok(rewritten)
    }
}
