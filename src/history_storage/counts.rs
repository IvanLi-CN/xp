use super::*;

pub(super) fn ensure_repository_history_counts(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction().map_err(sqlite_error)?;
    #[cfg(test)]
    if take_repository_history_counts_failure_for_test() {
        return Err(HistoryStorageError(
            "injected repository history counts migration failure".to_owned(),
        ));
    }
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS repository_history_counts (
                 id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
                 record_count INTEGER NOT NULL CHECK (record_count >= 0),
                 segment_count INTEGER NOT NULL CHECK (segment_count >= 0)
             );",
        )
        .map_err(sqlite_error)?;
    let initialized = transaction
        .query_row(
            "SELECT 1 FROM repository_history_counts WHERE id = 1",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(sqlite_error)?
        .is_some();
    if !initialized {
        transaction
            .execute(
                "INSERT INTO repository_history_counts (id, record_count, segment_count)
                 SELECT 1,
                        (SELECT COUNT(*) FROM repository_history_records),
                        (SELECT COUNT(*) FROM repository_history_segments)",
                [],
            )
            .map_err(sqlite_error)?;
    }
    transaction
        .execute_batch(
            "CREATE TRIGGER IF NOT EXISTS repository_history_records_count_insert
                 AFTER INSERT ON repository_history_records
                 BEGIN
                     UPDATE repository_history_counts
                        SET record_count = record_count + 1 WHERE id = 1;
                 END;
             CREATE TRIGGER IF NOT EXISTS repository_history_records_count_delete
                 AFTER DELETE ON repository_history_records
                 BEGIN
                     UPDATE repository_history_counts
                        SET record_count = record_count - 1 WHERE id = 1;
                 END;
             CREATE TRIGGER IF NOT EXISTS repository_history_segments_count_insert
                 AFTER INSERT ON repository_history_segments
                 BEGIN
                     UPDATE repository_history_counts
                        SET segment_count = segment_count + 1 WHERE id = 1;
                 END;
             CREATE TRIGGER IF NOT EXISTS repository_history_segments_count_delete
                 AFTER DELETE ON repository_history_segments
                 BEGIN
                     UPDATE repository_history_counts
                        SET segment_count = segment_count - 1 WHERE id = 1;
                 END;",
        )
        .map_err(sqlite_error)?;
    transaction.commit().map_err(sqlite_error)
}
