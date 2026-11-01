use std::collections::BTreeSet;

use super::*;

pub(super) fn bump_repository_history_segment_revisions(
    transaction: &rusqlite::Transaction<'_>,
    source_node_ids: &BTreeSet<String>,
) -> Result<()> {
    for source_node_id in source_node_ids {
        transaction
            .execute(
                "INSERT INTO repository_history_segment_revisions (source_node_id, revision)
                 VALUES (?1, 1)
                 ON CONFLICT(source_node_id) DO UPDATE SET revision = revision + 1",
                [source_node_id],
            )
            .map_err(sqlite_error)?;
    }
    Ok(())
}

pub(super) fn bump_repository_history_segment_revisions_for_expired_segments(
    transaction: &rusqlite::Transaction<'_>,
    closed_at_unix_seconds: u64,
) -> Result<()> {
    let closed_at = i64::try_from(closed_at_unix_seconds).unwrap_or(i64::MAX);
    transaction
        .execute(
            "UPDATE repository_history_segment_revisions
                SET revision = revision + 1
              WHERE EXISTS (
                  SELECT 1
                    FROM repository_history_segments
                   WHERE repository_history_segments.source_node_id =
                         repository_history_segment_revisions.source_node_id
                     AND closed_at < ?1
              )",
            [closed_at],
        )
        .map_err(sqlite_error)?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO repository_history_segment_revisions
                (source_node_id, revision)
             SELECT source_node_id, 1
               FROM repository_history_segments
              WHERE closed_at < ?1",
            [closed_at],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

pub(super) fn bump_repository_history_segment_revisions_for_all_segments(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<()> {
    transaction
        .execute(
            "UPDATE repository_history_segment_revisions
                SET revision = revision + 1
              WHERE EXISTS (
                  SELECT 1
                    FROM repository_history_segments
                   WHERE repository_history_segments.source_node_id =
                         repository_history_segment_revisions.source_node_id
              )",
            [],
        )
        .map_err(sqlite_error)?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO repository_history_segment_revisions
                (source_node_id, revision)
             SELECT source_node_id, 1
               FROM repository_history_segments",
            [],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

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
                "INSERT INTO repository_history_counts
                    (id, record_count, segment_count)
                 SELECT 1,
                        (SELECT COUNT(*) FROM repository_history_records),
                        (SELECT COUNT(*) FROM repository_history_segments)",
                [],
            )
            .map_err(sqlite_error)?;
    }
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS repository_history_segment_revisions (
                 source_node_id TEXT PRIMARY KEY NOT NULL,
                 revision INTEGER NOT NULL CHECK (revision >= 0)
             );
             INSERT OR IGNORE INTO repository_history_segment_revisions (source_node_id, revision)
                 SELECT source_node_id, 0
                   FROM repository_history_segments
                  GROUP BY source_node_id;",
        )
        .map_err(sqlite_error)?;
    transaction
        .execute_batch(
            "DROP TRIGGER IF EXISTS repository_history_segment_revision_insert;
             DROP TRIGGER IF EXISTS repository_history_segment_revision_delete;
             DROP TRIGGER IF EXISTS repository_history_segment_revision_update;
             DROP TRIGGER IF EXISTS repository_history_segment_revision_move;",
        )
        .map_err(sqlite_error)?;
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
