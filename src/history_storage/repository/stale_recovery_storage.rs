use super::*;

const MAX_STALE_STATE_BYTES: usize = 8 * 1024 * 1024;
const MAX_STALE_CONTROL_BYTES: usize = 16 * 1024 * 1024;
const MAX_STALE_PAGE_ROWS: usize = 128;
const MAX_STALE_PAGE_STORAGE_BYTES: usize = 4 * 192 * 1024 + 128 * 1024;

fn schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS repository_stale_recovery_state (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1), payload BLOB NOT NULL);",
        )
        .map_err(sqlite_error)
}

fn checkpoint(connection: &Connection) -> Result<Option<Vec<u8>>> {
    let size: Option<i64> = connection
        .query_row(
            "SELECT length(payload) FROM repository_stale_recovery_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)?;
    if size.is_some_and(|size| size < 0 || size as usize > MAX_STALE_STATE_BYTES) {
        return Err(HistoryStorageError(
            "stale recovery checkpoint exceeds bound".into(),
        ));
    }
    connection
        .query_row(
            "SELECT payload FROM repository_stale_recovery_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)
}

impl HistoryStorage {
    pub(crate) fn stale_recovery_checkpoint(&self) -> Result<Option<Vec<u8>>> {
        let mut backend = self.lock_backend();
        let connection = sqlite_connection(&mut backend)?
            .ok_or_else(|| HistoryStorageError("stale recovery requires SQLite".into()))?;
        schema(connection)?;
        checkpoint(connection)
    }

    /// Atomically commit a bounded deletion page, replica control and its recovery cursor.
    pub(crate) fn commit_repository_stale_recovery(
        &self,
        mutation: RepositoryReplicaMutation,
        control: &[u8],
        expected: Option<&[u8]>,
        next: &[u8],
    ) -> Result<RepositoryCommitOutcome> {
        let bytes = mutation
            .records
            .iter()
            .try_fold(0_usize, |total, row| total.checked_add(row.payload.len()))
            .and_then(|total| {
                mutation
                    .segments
                    .iter()
                    .try_fold(total, |n, row| n.checked_add(row.payload.len()))
            })
            .ok_or_else(|| HistoryStorageError("stale recovery page size overflow".into()))?;
        if control.len() > MAX_STALE_CONTROL_BYTES
            || next.len() > MAX_STALE_STATE_BYTES
            || mutation.records.len() > MAX_STALE_PAGE_ROWS
            || mutation.tombstones.len() > MAX_STALE_PAGE_ROWS
            || mutation.expired_tombstones.len() > 8192
            || mutation.segments.len() > MAX_STALE_PAGE_ROWS
            || mutation.applied_canonical_segments.len() > MAX_STALE_PAGE_ROWS
            || bytes > MAX_STALE_PAGE_STORAGE_BYTES
        {
            return Err(HistoryStorageError(
                "stale recovery transaction exceeds bound".into(),
            ));
        }
        self.commit_repository_replica_mutation_with(mutation, control, |transaction| {
            schema(transaction)?;
            if checkpoint(transaction)?.as_deref() != expected {
                return Err(HistoryStorageError(
                    "stale recovery checkpoint changed".into(),
                ));
            }
            transaction
                .execute(
                    "INSERT INTO repository_stale_recovery_state(singleton,payload) VALUES(1,?1)
                 ON CONFLICT(singleton) DO UPDATE SET payload=excluded.payload",
                    [next],
                )
                .map_err(sqlite_error)?;
            Ok(())
        })
    }
}
