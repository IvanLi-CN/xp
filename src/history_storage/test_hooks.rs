use super::{HistoryStorage, HistoryStorageError, Result, sqlite_connection};

impl HistoryStorage {
    pub(crate) fn set_query_only_for_test(&self, enabled: bool) -> Result<()> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Err(HistoryStorageError(
                "query-only test hook requires SQLite".to_owned(),
            ));
        };
        connection
            .pragma_update(None, "query_only", if enabled { "ON" } else { "OFF" })
            .map_err(super::sqlite_error)
    }

    pub(crate) fn set_maintenance_failure_for_test(&self, enabled: bool) {
        self.fail_maintenance_after_commit
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn set_history_rewrite_maintenance_failure_for_test(&self, enabled: bool) {
        self.fail_history_rewrite_maintenance
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }
}
