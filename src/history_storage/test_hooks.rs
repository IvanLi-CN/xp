use super::{HistoryStorage, HistoryStorageError, Result, sqlite_connection};

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static SLOW_LEGACY_HISTORY_STATUS_COUNT: AtomicBool = AtomicBool::new(false);

pub(crate) struct SlowLegacyHistoryStatusCountGuard;

pub(crate) fn slow_legacy_history_status_count_for_test() -> SlowLegacyHistoryStatusCountGuard {
    SLOW_LEGACY_HISTORY_STATUS_COUNT.store(true, Ordering::SeqCst);
    SlowLegacyHistoryStatusCountGuard
}

impl Drop for SlowLegacyHistoryStatusCountGuard {
    fn drop(&mut self) {
        SLOW_LEGACY_HISTORY_STATUS_COUNT.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn maybe_delay_legacy_history_count_for_test(caller_class: &'static str, query: &str) {
    // Model the old synchronous index scan without slowing the fixed COUNT(*) path.
    if SLOW_LEGACY_HISTORY_STATUS_COUNT.load(Ordering::SeqCst)
        && caller_class == "http.internal_history_repository_status"
        && query.contains("INDEXED BY")
    {
        std::thread::sleep(Duration::from_millis(250));
    }
}

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
