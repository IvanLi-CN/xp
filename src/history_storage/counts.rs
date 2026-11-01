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
                 segment_count INTEGER NOT NULL CHECK (segment_count >= 0),
                 segment_revision INTEGER NOT NULL CHECK (segment_revision >= 0)
             );",
        )
        .map_err(sqlite_error)?;
    let columns = {
        let mut statement = transaction
            .prepare("PRAGMA table_info(repository_history_counts)")
            .map_err(sqlite_error)?;
        statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(sqlite_error)?
            .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()
            .map_err(sqlite_error)?
    };
    if !columns.contains("segment_revision") {
        transaction
            .execute(
                "ALTER TABLE repository_history_counts
                 ADD COLUMN segment_revision INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(sqlite_error)?;
    }
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
                "INSERT INTO repository_history_counts
                    (id, record_count, segment_count, segment_revision)
                 SELECT 1,
                        (SELECT COUNT(*) FROM repository_history_records),
                        (SELECT COUNT(*) FROM repository_history_segments),
                        0",
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
                 END;
             CREATE TRIGGER IF NOT EXISTS repository_history_segments_revision_insert
                 AFTER INSERT ON repository_history_segments
                 BEGIN
                     UPDATE repository_history_counts
                        SET segment_revision = segment_revision + 1 WHERE id = 1;
                 END;
             CREATE TRIGGER IF NOT EXISTS repository_history_segments_revision_delete
                 AFTER DELETE ON repository_history_segments
                 BEGIN
                     UPDATE repository_history_counts
                        SET segment_revision = segment_revision + 1 WHERE id = 1;
                 END;
             CREATE TRIGGER IF NOT EXISTS repository_history_segments_revision_update
                 AFTER UPDATE OF closed_at, contains_tombstone, source_node_id, source_epoch,
                     stream, first_sequence, payload ON repository_history_segments
                 BEGIN
                     UPDATE repository_history_counts
                        SET segment_revision = segment_revision + 1 WHERE id = 1;
                 END;",
        )
        .map_err(sqlite_error)?;
    transaction.commit().map_err(sqlite_error)
}
