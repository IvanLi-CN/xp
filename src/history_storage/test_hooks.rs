use super::{HistoryStorage, HistoryStorageError, Result, sqlite_connection};

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static SLOW_LEGACY_HISTORY_STATUS_COUNT: AtomicBool = AtomicBool::new(false);
static FORCE_LEGACY_HISTORY_STATUS_COUNT: AtomicBool = AtomicBool::new(false);
static HISTORY_STATUS_COUNT_STARTED: AtomicBool = AtomicBool::new(false);
static HISTORY_STATUS_COUNT_USED_FIXED_QUERY: AtomicBool = AtomicBool::new(false);
static HISTORY_STATUS_COUNT_USED_LEGACY_QUERY: AtomicBool = AtomicBool::new(false);

pub(crate) struct LegacyHistoryStatusCountGuard;
pub(crate) struct HistoryStatusCountProbeGuard;

pub(crate) fn legacy_history_status_count_for_test() -> LegacyHistoryStatusCountGuard {
    FORCE_LEGACY_HISTORY_STATUS_COUNT.store(true, Ordering::SeqCst);
    SLOW_LEGACY_HISTORY_STATUS_COUNT.store(true, Ordering::SeqCst);
    LegacyHistoryStatusCountGuard
}

impl Drop for LegacyHistoryStatusCountGuard {
    fn drop(&mut self) {
        FORCE_LEGACY_HISTORY_STATUS_COUNT.store(false, Ordering::SeqCst);
        SLOW_LEGACY_HISTORY_STATUS_COUNT.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn history_status_count_probe_for_test() -> HistoryStatusCountProbeGuard {
    HISTORY_STATUS_COUNT_STARTED.store(false, Ordering::SeqCst);
    HISTORY_STATUS_COUNT_USED_FIXED_QUERY.store(false, Ordering::SeqCst);
    HISTORY_STATUS_COUNT_USED_LEGACY_QUERY.store(false, Ordering::SeqCst);
    HistoryStatusCountProbeGuard
}

impl Drop for HistoryStatusCountProbeGuard {
    fn drop(&mut self) {
        HISTORY_STATUS_COUNT_STARTED.store(false, Ordering::SeqCst);
        HISTORY_STATUS_COUNT_USED_FIXED_QUERY.store(false, Ordering::SeqCst);
        HISTORY_STATUS_COUNT_USED_LEGACY_QUERY.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn history_status_count_started_for_test() -> bool {
    HISTORY_STATUS_COUNT_STARTED.load(Ordering::SeqCst)
}

pub(crate) fn history_status_count_used_fixed_query_for_test() -> bool {
    HISTORY_STATUS_COUNT_USED_FIXED_QUERY.load(Ordering::SeqCst)
}

pub(crate) fn history_status_count_used_legacy_query_for_test() -> bool {
    HISTORY_STATUS_COUNT_USED_LEGACY_QUERY.load(Ordering::SeqCst)
}

pub(crate) fn history_status_count_query_for_test(
    table: &str,
    caller_class: &'static str,
    query: &'static str,
) -> &'static str {
    if caller_class == "http.internal_history_repository_status" {
        let selected_query = if FORCE_LEGACY_HISTORY_STATUS_COUNT.load(Ordering::SeqCst) {
            match table {
                "repository_history_records" => {
                    "SELECT COUNT(source_node_id) FROM repository_history_records
                     INDEXED BY repository_history_records_keyset"
                }
                "repository_history_segments" => {
                    "SELECT COUNT(id) FROM repository_history_segments
                     INDEXED BY repository_history_segments_sync_order_v2"
                }
                _ => query,
            }
        } else {
            query
        };
        if selected_query.contains("INDEXED BY") {
            HISTORY_STATUS_COUNT_USED_LEGACY_QUERY.store(true, Ordering::SeqCst);
        } else if selected_query.contains("repository_history_counts") {
            HISTORY_STATUS_COUNT_USED_FIXED_QUERY.store(true, Ordering::SeqCst);
        }
        selected_query
    } else {
        query
    }
}

pub(crate) fn maybe_delay_legacy_history_count_for_test(caller_class: &'static str, query: &str) {
    // Model the old synchronous index scan without slowing the materialized count path.
    if caller_class == "http.internal_history_repository_status" {
        HISTORY_STATUS_COUNT_STARTED.store(true, Ordering::SeqCst);
    }
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
