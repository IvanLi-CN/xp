use super::*;

fn schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS node_history_deletion_guard (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            version INTEGER NOT NULL, digest BLOB NOT NULL);",
        )
        .map_err(sqlite_error)
}

fn version_number(version: u64) -> Result<i64> {
    i64::try_from(version)
        .map_err(|_| HistoryStorageError("node deletion version exhausted".into()))
}

fn snapshot_version(connection: &Connection) -> Result<Option<i64>> {
    connection
        .query_row(
            "SELECT COALESCE(json_extract(CAST(payload AS TEXT), '$.deletion_version'), 0)
         FROM history_snapshots WHERE key = ?1",
            [NODE_HISTORY_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)
}

fn current_guard(connection: &Connection) -> Result<Option<(i64, Vec<u8>)>> {
    connection
        .query_row(
            "SELECT version, digest FROM node_history_deletion_guard
         WHERE singleton = 1 AND length(digest) = 32",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sqlite_error)
}

impl HistoryStorage {
    /// Prove a specific removal in the saved membership operation and saved node set.
    pub(crate) fn saved_state_confirms_node_removal(
        &self,
        node_id: &str,
        operation_id: &str,
    ) -> Result<bool> {
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("durable node membership requires SQLite".into()))?;
        let state: Option<(Option<String>, bool, bool)> = connection
            .query_row(
                "SELECT json_type(CAST(payload AS TEXT), '$.nodes'),
                    EXISTS(SELECT 1 FROM json_each(
                           CAST(history_snapshots.payload AS TEXT), '$.nodes')
                           WHERE key = ?2),
                    EXISTS(SELECT 1 FROM json_each(
                           CAST(history_snapshots.payload AS TEXT), '$.membership_operations')
                           WHERE key = ?3 AND json_extract(value, '$.operation_id') = ?3
                             AND json_extract(value, '$.node_id') = ?2
                             AND json_extract(value, '$.kind') = 'remove_node'
                             AND json_extract(value, '$.phase')
                                 IN ('membership_removed', 'completed'))
                 FROM history_snapshots WHERE key = ?1",
                params![STATE_KEY, node_id, operation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(sqlite_error)?;
        match state {
            Some((Some(kind), present, removal)) if kind == "object" => Ok(!present && removal),
            _ => Err(HistoryStorageError(
                "durable cluster membership is unavailable".into(),
            )),
        }
    }

    /// Check the saved cluster snapshot, rather than a possibly uncommitted in-memory mutation.
    pub(crate) fn saved_state_lacks_node(&self, node_id: &str) -> Result<bool> {
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("durable node membership requires SQLite".into()))?;
        let state: Option<(Option<String>, bool)> = connection
            .query_row(
                "SELECT json_type(CAST(payload AS TEXT), '$.nodes'), EXISTS(
                    SELECT 1 FROM json_each(CAST(history_snapshots.payload AS TEXT), '$.nodes')
                    WHERE key = ?2
                 ) FROM history_snapshots WHERE key = ?1",
                params![STATE_KEY, node_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(sqlite_error)?;
        match state {
            Some((Some(kind), present)) if kind == "object" => Ok(!present),
            _ => Err(HistoryStorageError(
                "durable cluster membership is unavailable".into(),
            )),
        }
    }

    /// Commit deletion version and node history in one SQLite transaction without JSON fallback.
    pub(crate) fn write_node_history_guarded(
        &self,
        payload: &[u8],
        version: u64,
        digest: &[u8; 32],
    ) -> Result<()> {
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("guarded node history requires SQLite".into()))?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        schema(&transaction)?;
        let version = version_number(version)?;
        if let Some((previous, previous_digest)) = current_guard(&transaction)? {
            if version < previous || (version == previous && previous_digest != digest.as_slice()) {
                return Err(HistoryStorageError(
                    "node deletion version conflicts with durable state".into(),
                ));
            }
            if snapshot_version(&transaction)?.unwrap_or(0) != previous {
                return Err(HistoryStorageError(
                    "node deletion snapshot version diverged".into(),
                ));
            }
        } else if snapshot_version(&transaction)?.unwrap_or(0) > version {
            return Err(HistoryStorageError(
                "node deletion version would roll back".into(),
            ));
        }
        transaction
            .execute(
                "INSERT INTO node_history_deletion_guard(singleton,version,digest) VALUES(1,?1,?2)
             ON CONFLICT(singleton) DO UPDATE SET version=excluded.version,digest=excluded.digest",
                params![version, digest.as_slice()],
            )
            .map_err(sqlite_error)?;
        write_snapshot(&transaction, NODE_HISTORY_KEY, payload)?;
        transaction.commit().map_err(sqlite_error)?;
        finish_post_commit_maintenance(maintain_sqlite(connection));
        Ok(())
    }

    /// Initialize legacy metadata once, then prove the in-memory state matches SQLite.
    pub(crate) fn check_node_history_guard(&self, version: u64, digest: &[u8; 32]) -> Result<()> {
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("guarded node history requires SQLite".into()))?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        schema(&transaction)?;
        let version = version_number(version)?;
        if snapshot_version(&transaction)?.unwrap_or(0) != version {
            return Err(HistoryStorageError(
                "node deletion snapshot version diverged".into(),
            ));
        }
        match current_guard(&transaction)? {
            Some((stored, hash)) if stored == version && hash == digest.as_slice() => {}
            Some(_) => return Err(HistoryStorageError("node deletion guard changed".into())),
            None => {
                transaction
                    .execute(
                        "INSERT INTO node_history_deletion_guard(singleton,version,digest) \
                     VALUES(1,?1,?2)",
                        params![version, digest.as_slice()],
                    )
                    .map_err(sqlite_error)?;
            }
        }
        transaction.commit().map_err(sqlite_error)?;
        Ok(())
    }
}
